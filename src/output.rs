use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;

use crate::document::{Document, PageStatus};
use crate::enums::OcrMode;
use crate::filter::Exclusion;

/// Writes all raw, filtered, exclusion, and processing artifacts.
/// Returns the number of OCR-failed pages for the caller's final decision.
pub fn write_document_outputs(
    out: &Utf8PathBuf,
    document: &Document,
    exclusions: &[Exclusion],
) -> Result<usize> {
    let report = processing_report(document);
    std::fs::create_dir_all(out.join("debug"))?;
    write_json(&out.join("debug/exclusions.json"), exclusions)?;
    write_json(&out.join("debug/processing.json"), &report)?;

    for page in &document.pages {
        write_page(out, page.index, page.raw_output_mode, &page.raw_output)?;
        write_filtered_page(out, page.index, &page.filtered_text())?;
        println!("wrote page {}", page.index + 1);
    }

    Ok(report.failures.len())
}

#[derive(Serialize)]
struct ProcessingReport {
    failures: Vec<ProcessingEntry>,
    warnings: Vec<ProcessingEntry>,
}

#[derive(Serialize)]
struct ProcessingEntry {
    page: usize,
    path: String,
    message: String,
}

fn processing_report(document: &Document) -> ProcessingReport {
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    for page in &document.pages {
        if let PageStatus::OcrFailed { error } = &page.status {
            failures.push(ProcessingEntry {
                page: page.index + 1,
                path: page.path.clone(),
                message: error.clone(),
            });
        }
        warnings.extend(page.warnings.iter().map(|message| ProcessingEntry {
            page: page.index + 1,
            path: page.path.clone(),
            message: message.clone(),
        }));
    }
    ProcessingReport { failures, warnings }
}

fn write_json<T: Serialize + ?Sized>(path: &Utf8Path, value: &T) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn write_filtered_page(out: &Utf8PathBuf, index: usize, text: &str) -> Result<Utf8PathBuf> {
    let output_dir = out.join("filtered");
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create filtered output directory {output_dir}"))?;
    let output_path = output_dir.join(format!("{:04}.txt", index + 1));
    write_text_to_file(text.as_bytes(), &output_path)?;
    Ok(output_path)
}

fn write_text_to_file(text: &[u8], output_path: &Utf8PathBuf) -> Result<()> {
    std::fs::write(output_path, text)
        .with_context(|| format!("failed to write text to {output_path}"))
}

fn write_page(out: &Utf8PathBuf, index: usize, mode: OcrMode, text: &str) -> Result<Utf8PathBuf> {
    let filename = format!("{:04}.{}", index + 1, mode.extension());
    let output_path = out.join(filename);
    write_text_to_file(text.as_bytes(), &output_path)?;
    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Block, Page};
    use crate::filter::filter_document;

    fn success_page(index: usize, body: &str) -> Page {
        let blocks = vec![
            Block::new(1, 1, "Repeated header", 100, 40, 300, 20),
            Block::new(2, 1, body, 100, 400, 500, 30),
        ];
        Page {
            index,
            path: format!("{index}.png"),
            status: PageStatus::OcrSucceeded,
            raw_text: format!("Repeated header\n{body}\n"),
            raw_output: format!("Repeated header\n{body}\n"),
            raw_output_mode: OcrMode::Text,
            filtered_blocks: blocks.clone(),
            blocks,
            page_width: 1000,
            page_height: 1000,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn writes_page_indexed_artifacts_around_a_failed_middle_page() {
        let output = std::env::temp_dir().join(format!(
            "afterprint-p0-1-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let output = Utf8PathBuf::from_path_buf(output).unwrap();
        let mut document = Document {
            pages: vec![
                success_page(0, "first page"),
                Page {
                    index: 1,
                    path: "1.png".to_string(),
                    status: PageStatus::OcrFailed {
                        error: "OCR failed".to_string(),
                    },
                    raw_text: String::new(),
                    raw_output: String::new(),
                    raw_output_mode: OcrMode::Text,
                    blocks: Vec::new(),
                    filtered_blocks: Vec::new(),
                    page_width: 0,
                    page_height: 0,
                    warnings: Vec::new(),
                },
                success_page(2, "third page"),
            ],
        };
        let exclusions = filter_document(&mut document);

        assert_eq!(
            write_document_outputs(&output, &document, &exclusions).unwrap(),
            1
        );

        for name in ["0001.txt", "0002.txt", "0003.txt"] {
            assert!(output.join(name).exists());
            assert!(output.join("filtered").join(name).exists());
        }
        assert_eq!(
            std::fs::read_to_string(output.join("0001.txt")).unwrap(),
            "Repeated header\nfirst page\n"
        );
        assert!(
            std::fs::read_to_string(output.join("0002.txt"))
                .unwrap()
                .is_empty()
        );
        assert!(
            !std::fs::read_to_string(output.join("filtered/0001.txt"))
                .unwrap()
                .contains("Repeated header")
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(output.join("debug/exclusions.json")).unwrap()
            )
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(output.join("debug/processing.json")).unwrap()
            )
            .unwrap()["failures"][0]["page"],
            2
        );
        assert_eq!(
            document
                .pages
                .iter()
                .map(|page| page.index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        std::fs::remove_dir_all(output).unwrap();
    }
}
