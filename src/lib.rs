use anyhow::{Context, Result, ensure};
use camino::Utf8PathBuf;
use document::{Document, Page, PageStatus};
use filter::filter_document;
use ocr::{OcrEvent, ocr_pages_from_paths, supported_image_paths};
use output::write_document_outputs;
use pdf::write_pdf;
use segments::{build_segments, map_translations_to_pages};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use translation::{Ct2Translator, write_translations};

pub mod cli;
mod document;
mod enums;
mod filter;
mod ocr;
mod output;
mod pdf;
mod segments;
mod translation;

pub use enums::{OcrMode, TranslationLanguage, TranslationModel};
pub use pdf::{PdfContent, SOURCE_PDF_FILENAME, TRANSLATED_PDF_FILENAME};
pub use translation::{ModelReadiness, TranslationConfig};

const EVENT_SCHEMA_VERSION: u8 = 1;

/// Complete settings for one run of the existing afterprint pipeline.
#[derive(Clone, Debug)]
pub struct RunConfig {
    pub input: PathBuf,
    pub output: PathBuf,
    pub ocr_language: String,
    pub ocr_mode: OcrMode,
    pub title: String,
    pub author: String,
    pub language: String,
    pub pdf: bool,
    pub pdf_content: PdfContent,
    pub translation: Option<TranslationConfig>,
}

/// Processing stages reported by [`RunEvent::Stage`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStage {
    Ocr,
    Filter,
    Translate,
    WriteOutputs,
}

/// Final state of a run. OCR page failures produce `PartialFailure` after
/// available artifacts have been written; fatal setup or output errors return
/// `Err` and emit `Failure`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Success,
    PartialFailure,
    Failure,
}

/// Typed progress, warning, failure, artifact, and completion notifications.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RunEvent {
    Stage {
        stage: RunStage,
    },
    PageProgress {
        page: usize,
        total: usize,
    },
    Warning {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        page: Option<usize>,
    },
    PageFailed {
        page: usize,
        message: String,
    },
    SetupProgress {
        component: String,
        stage: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        completed: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    ArtifactWritten {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        page: Option<usize>,
    },
    RunFinished {
        status: RunStatus,
        pages_total: usize,
        pages_succeeded: usize,
        pages_failed: usize,
    },
}

impl RunEvent {
    /// Serializes one versioned JSON Lines record, without the trailing newline.
    pub fn to_jsonl(&self) -> std::result::Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct VersionedEvent<'a> {
            schema_version: u8,
            #[serde(flatten)]
            event: &'a RunEvent,
        }

        serde_json::to_string(&VersionedEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            event: self,
        })
    }
}

/// One output path written during a completed run. `path` is relative to the
/// configured output folder and `page` uses one-based indexing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artifact {
    pub path: String,
    pub page: Option<usize>,
}

/// Summary of a completed run, including any OCR page failures and artifacts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunReport {
    pub status: RunStatus,
    pub pages_total: usize,
    pub pages_succeeded: usize,
    pub pages_failed: usize,
    pub artifacts: Vec<Artifact>,
}

/// Runs with models that are already prepared. Call `prepare_translation_model`
/// explicitly before this when readiness reports a missing managed model.
pub fn run(config: &RunConfig, emit: impl FnMut(RunEvent)) -> Result<RunReport> {
    run_pipeline(config, emit, TranslationConfig::open_ready_translator)
}

/// CLI adapter that retains the existing interactive model preparation flow.
pub fn run_cli(config: &RunConfig, emit: impl FnMut(RunEvent)) -> Result<RunReport> {
    run_pipeline(config, emit, TranslationConfig::open_translator_for_cli)
}

fn run_pipeline(
    config: &RunConfig,
    mut emit: impl FnMut(RunEvent),
    open_translation_model: impl FnOnce(&TranslationConfig) -> Result<Ct2Translator>,
) -> Result<RunReport> {
    let mut counts = RunCounts::default();
    let result = run_scan_folder(config, &mut emit, &mut counts, open_translation_model);
    let status = match &result {
        Ok(report) => report.status,
        Err(_) => RunStatus::Failure,
    };
    emit(RunEvent::RunFinished {
        status,
        pages_total: counts.total,
        pages_succeeded: counts.succeeded,
        pages_failed: counts.failed,
    });
    result
}

fn run_scan_folder(
    config: &RunConfig,
    emit: &mut impl FnMut(RunEvent),
    counts: &mut RunCounts,
    open_translation_model: impl FnOnce(&TranslationConfig) -> Result<Ct2Translator>,
) -> Result<RunReport> {
    let (output, paths) = validate_run_config(config)?;
    counts.total = paths.len();
    let translator = config
        .translation
        .as_ref()
        .map(open_translation_model)
        .transpose()?;

    emit(RunEvent::Stage {
        stage: RunStage::Ocr,
    });
    let mut pages = Vec::with_capacity(paths.len());
    for event in ocr_pages_from_paths(paths, &config.ocr_language, config.ocr_mode) {
        match event {
            OcrEvent::PageProgress { page, total } => {
                emit(RunEvent::PageProgress { page, total });
            }
            OcrEvent::Page(page) => {
                record_page_result(&page, counts, emit);
                pages.push(page);
            }
        }
    }

    process_document(
        DocumentRun {
            config,
            output: &output,
            translator,
            counts,
        },
        Document { pages },
        emit,
    )
}

fn record_page_result(page: &Page, counts: &mut RunCounts, emit: &mut impl FnMut(RunEvent)) {
    match &page.status {
        PageStatus::OcrSucceeded => counts.succeeded += 1,
        PageStatus::OcrFailed { error } => {
            counts.failed += 1;
            emit(RunEvent::PageFailed {
                page: page.index + 1,
                message: error.clone(),
            });
        }
    }
    for warning in &page.warnings {
        emit(RunEvent::Warning {
            page: Some(page.index + 1),
            message: warning.clone(),
        });
    }
}

struct DocumentRun<'a> {
    config: &'a RunConfig,
    output: &'a Utf8PathBuf,
    translator: Option<Ct2Translator>,
    counts: &'a RunCounts,
}

fn process_document(
    DocumentRun {
        config,
        output,
        translator,
        counts,
    }: DocumentRun<'_>,
    mut document: Document,
    emit: &mut impl FnMut(RunEvent),
) -> Result<RunReport> {
    emit(RunEvent::Stage {
        stage: RunStage::Filter,
    });
    let exclusions = filter_document(&mut document);
    let segments = build_segments(&document);
    let mut artifacts = Vec::new();

    emit(RunEvent::Stage {
        stage: RunStage::WriteOutputs,
    });
    write_document_outputs(output, &document, &exclusions, &segments, |path, page| {
        record_artifact(&mut artifacts, emit, path, page);
    })?;

    if config.pdf {
        let page_texts = document
            .pages
            .iter()
            .map(|page| config.pdf_content.text(page))
            .collect::<Vec<_>>();
        write_pdf(
            output,
            SOURCE_PDF_FILENAME,
            &document,
            &page_texts,
            &config.title,
            &config.author,
            &config.language,
        )?;
        record_artifact(&mut artifacts, emit, SOURCE_PDF_FILENAME.into(), None);
    }

    if let Some(translator) = translator {
        emit(RunEvent::Stage {
            stage: RunStage::Translate,
        });
        let sources = segments
            .iter()
            .map(|segment| segment.source_text.clone())
            .collect::<Vec<_>>();
        let translations = translator.translate_segments(&sources)?;
        let page_translations =
            map_translations_to_pages(&segments, &translations, document.pages.len())?;

        emit(RunEvent::Stage {
            stage: RunStage::WriteOutputs,
        });
        for page in &document.pages {
            let source = page.filtered_text();
            let translation = page_translations
                .get(page.index)
                .map(String::as_str)
                .unwrap_or_default();
            if source.trim().is_empty() || translation.trim().is_empty() {
                continue;
            }

            match write_translations(output, page.index, source.trim_end(), translation) {
                Ok(_) => record_artifact(
                    &mut artifacts,
                    emit,
                    format!("translations/{:04}.txt", page.index + 1),
                    Some(page.index + 1),
                ),
                Err(error) => emit(RunEvent::Warning {
                    page: Some(page.index + 1),
                    message: format!(
                        "failed to write translations page {}. Err: {error}",
                        page.index + 1
                    ),
                }),
            }
        }

        if config.pdf {
            write_pdf(
                output,
                TRANSLATED_PDF_FILENAME,
                &document,
                &page_translations,
                &config.title,
                &config.author,
                &config.language,
            )?;
            record_artifact(&mut artifacts, emit, TRANSLATED_PDF_FILENAME.into(), None);
        }
    }

    let status = if counts.failed == 0 {
        RunStatus::Success
    } else {
        RunStatus::PartialFailure
    };
    Ok(RunReport {
        status,
        pages_total: counts.total,
        pages_succeeded: counts.succeeded,
        pages_failed: counts.failed,
        artifacts,
    })
}

fn record_artifact(
    artifacts: &mut Vec<Artifact>,
    emit: &mut impl FnMut(RunEvent),
    path: String,
    page: Option<usize>,
) {
    artifacts.push(Artifact {
        path: path.clone(),
        page,
    });
    emit(RunEvent::ArtifactWritten { path, page });
}

/// Checks a configured custom or managed translation model without prompting
/// or downloading files.
pub fn translation_model_readiness(config: &TranslationConfig) -> Result<ModelReadiness> {
    config.readiness()
}

/// Explicitly prepares a managed model without terminal input and forwards
/// setup progress through the shared event type.
pub fn prepare_translation_model(
    config: &TranslationConfig,
    mut emit: impl FnMut(RunEvent),
) -> Result<ModelReadiness> {
    config.prepare(|progress| {
        emit(RunEvent::SetupProgress {
            component: progress.component,
            stage: progress.stage,
            message: progress.message,
            completed: progress.completed,
            total: progress.total,
        });
    })
}

#[derive(Default)]
struct RunCounts {
    total: usize,
    succeeded: usize,
    failed: usize,
}

fn validate_run_config(config: &RunConfig) -> Result<(Utf8PathBuf, Vec<PathBuf>)> {
    let input_resolved = resolve_path(&config.input)?;
    ensure!(input_resolved.is_dir(), "scan folder is not a directory");
    let output_resolved = resolve_path(&config.output)?;
    ensure!(
        !output_resolved.starts_with(&input_resolved),
        "output folder must be outside the scan folder"
    );

    let input = Utf8PathBuf::from_path_buf(config.input.clone())
        .map_err(|_| anyhow::anyhow!("scan folder path must be valid UTF-8"))?;
    let output = Utf8PathBuf::from_path_buf(config.output.clone())
        .map_err(|_| anyhow::anyhow!("output folder path must be valid UTF-8"))?;
    let paths = supported_image_paths(input.as_std_path());
    ensure!(
        !paths.is_empty(),
        "scan folder contains no supported images (bmp, gif, jpeg, jpg, pbm, pgm, png, ppm, tif, tiff, webp)"
    );
    Ok((output, paths))
}

fn resolve_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => resolved.push(prefix.as_os_str()),
            Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                let next = resolved.join(name);
                match std::fs::symlink_metadata(&next) {
                    Ok(_) => {
                        resolved = std::fs::canonicalize(&next)
                            .with_context(|| format!("failed to resolve path {}", next.display()))?
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        resolved.push(name)
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use enums::OcrMode;

    #[test]
    fn jsonl_events_have_the_versioned_required_fields() {
        let event = RunEvent::RunFinished {
            status: RunStatus::PartialFailure,
            pages_total: 3,
            pages_succeeded: 2,
            pages_failed: 1,
        };

        let value: serde_json::Value = serde_json::from_str(&event.to_jsonl().unwrap()).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["event"], "run_finished");
        assert_eq!(value["status"], "partial_failure");
        assert_eq!(value["pages_total"], 3);
        assert_eq!(value["pages_succeeded"], 2);
        assert_eq!(value["pages_failed"], 1);
    }

    #[test]
    fn synthetic_library_run_reports_pages_warnings_artifacts_and_partial_failure() {
        let root = temp_dir("library-run");
        let output = root.join("output");
        let output_utf8 = Utf8PathBuf::from_path_buf(output.clone()).unwrap();
        let config = test_config(root.join("input"), output.clone());
        let pages = vec![
            page(
                0,
                "first page",
                PageStatus::OcrSucceeded,
                vec!["layout fallback"],
            ),
            page(
                1,
                "",
                PageStatus::OcrFailed {
                    error: "invalid image".into(),
                },
                vec![],
            ),
            page(2, "third page", PageStatus::OcrSucceeded, vec![]),
        ];
        let mut counts = RunCounts {
            total: pages.len(),
            ..RunCounts::default()
        };
        let mut events = Vec::new();
        for page in &pages {
            events.push(RunEvent::PageProgress {
                page: page.index + 1,
                total: pages.len(),
            });
            record_page_result(page, &mut counts, &mut |event| events.push(event));
        }
        let report = process_document(
            DocumentRun {
                config: &config,
                output: &output_utf8,
                translator: None,
                counts: &counts,
            },
            Document { pages },
            &mut |event| events.push(event),
        )
        .unwrap();
        events.push(RunEvent::RunFinished {
            status: report.status,
            pages_total: report.pages_total,
            pages_succeeded: report.pages_succeeded,
            pages_failed: report.pages_failed,
        });

        assert_eq!(report.status, RunStatus::PartialFailure);
        assert_eq!(
            (
                report.pages_total,
                report.pages_succeeded,
                report.pages_failed
            ),
            (3, 2, 1)
        );
        assert!(events.iter().any(|event| matches!(
            event,
            RunEvent::Warning { page: Some(1), message } if message == "layout fallback"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            RunEvent::PageFailed { page: 2, message } if message == "invalid image"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            RunEvent::ArtifactWritten { path, page: Some(1) } if path == "0001.txt"
        )));
        assert!(matches!(
            events.last(),
            Some(RunEvent::RunFinished {
                status: RunStatus::PartialFailure,
                pages_total: 3,
                pages_succeeded: 2,
                pages_failed: 1,
            })
        ));
        assert!(output.join("0002.txt").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validation_rejects_empty_input_and_output_inside_input_before_writing() {
        let root = temp_dir("validation");
        let input = root.join("input");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::write(input.join("notes.txt"), "unsupported").unwrap();
        assert!(
            validate_run_config(&test_config(input.clone(), root.join("output")))
                .unwrap_err()
                .to_string()
                .contains("no supported images")
        );

        std::fs::write(input.join("001.pgm"), []).unwrap();
        assert!(
            validate_run_config(&test_config(input.clone(), input.join("output")))
                .unwrap_err()
                .to_string()
                .contains("outside the scan folder")
        );
        assert!(
            validate_run_config(&test_config(input.clone(), input.clone()))
                .unwrap_err()
                .to_string()
                .contains("outside the scan folder")
        );
        assert!(!input.join("output").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn validation_rejects_output_symlink_resolving_inside_input() {
        let root = temp_dir("symlink-validation");
        let input = root.join("input");
        let nested = input.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(input.join("001.pgm"), []).unwrap();
        let link = root.join("output-link");
        std::os::unix::fs::symlink(&nested, &link).unwrap();

        let error = validate_run_config(&test_config(input, link)).unwrap_err();
        assert!(error.to_string().contains("outside the scan folder"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn resolves_existing_symlinks_and_nonexistent_output_suffixes() {
        let root = temp_dir("resolve-path");
        let input = root.join("input");
        let nested = input.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let link = root.join("outside-link");
        std::os::unix::fs::symlink(&nested, &link).unwrap();

        assert_eq!(
            resolve_path(&link).unwrap(),
            std::fs::canonicalize(nested).unwrap()
        );
        assert_eq!(
            resolve_path(&input.join("new").join("output")).unwrap(),
            std::fs::canonicalize(&input).unwrap().join("new/output")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "afterprint-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn test_config(input: PathBuf, output: PathBuf) -> RunConfig {
        RunConfig {
            input,
            output,
            ocr_language: "eng".into(),
            ocr_mode: OcrMode::Text,
            title: "Test".into(),
            author: "Test".into(),
            language: "en".into(),
            pdf: false,
            pdf_content: PdfContent::Filtered,
            translation: None,
        }
    }

    fn page(index: usize, text: &str, status: PageStatus, warnings: Vec<&str>) -> Page {
        Page {
            index,
            path: format!("page-{}.png", index + 1),
            status,
            raw_text: text.into(),
            raw_output: text.into(),
            raw_output_mode: OcrMode::Text,
            blocks: Vec::new(),
            filtered_blocks: Vec::new(),
            page_width: 0,
            page_height: 0,
            warnings: warnings.into_iter().map(str::to_owned).collect(),
        }
    }
}
