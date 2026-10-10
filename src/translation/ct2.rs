use anyhow::{Context, Result, bail, ensure};
use ct2rs::{
    ComputeType, Config, TranslationOptions, Translator as CTranslateTranslator,
    tokenizers::auto::Tokenizer as AutoTokenizer,
};
use std::{
    env, fs,
    io::{self, BufRead, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use crate::enums::{TranslationLanguage, TranslationModel};

use super::{opus_pairs, translate_segments};

const CT2_MAX_TRANSLATION_UNIT_BYTES: usize = 800;
const NLLB_MODEL_ID: &str = "facebook/nllb-200-distilled-600M";
static NEXT_MODEL_DOWNLOAD_ATTEMPT: AtomicU64 = AtomicU64::new(0);

type AutoTranslator = CTranslateTranslator<AutoTokenizer>;

#[derive(Clone, Debug)]
/// Translation settings independent of Clap or interactive terminal input.
pub struct TranslationConfig {
    pub model: TranslationModel,
    pub source: Option<TranslationLanguage>,
    pub target: Option<TranslationLanguage>,
    pub model_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Readiness result for a configured custom or managed translation model.
pub struct ModelReadiness {
    pub ready: bool,
    pub model_path: PathBuf,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SetupProgress {
    pub component: String,
    pub stage: String,
    pub message: String,
    pub completed: Option<u64>,
    pub total: Option<u64>,
}

impl TranslationConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        self.resolve_translation_request().map(|_| ())
    }

    fn resolve_translation_request(&self) -> Result<TranslationRequest<'_>> {
        match (
            self.model,
            self.model_path.as_deref(),
            self.source,
            self.target,
        ) {
            (TranslationModel::Opus, Some(path), None, None) => {
                Ok(TranslationRequest::CustomOpus { path })
            }
            (TranslationModel::Opus, Some(_), _, _) => bail!(
                "a custom OPUS model defines its own language direction; omit \
                 --translation-source and --translation-target"
            ),
            (TranslationModel::Nllb, Some(path), Some(source), Some(target)) => {
                Ok(TranslationRequest::CustomNllb {
                    path,
                    pair: LanguagePair::new(source, target)?,
                })
            }
            (_, None, Some(source), Some(target)) => Ok(TranslationRequest::ManagedPair {
                model: self.model,
                pair: LanguagePair::new(source, target)?,
            }),
            (TranslationModel::Nllb, Some(_), _, _) => {
                bail!("a custom NLLB model requires --translation-source and --translation-target")
            }
            (_, None, _, _) => bail!(
                "--translation-source and --translation-target are required for a managed model"
            ),
        }
    }

    pub(crate) fn readiness(&self) -> Result<ModelReadiness> {
        match self.resolve_translation_request()? {
            TranslationRequest::ManagedPair { model, pair } => {
                let spec = model_spec(model, pair)?;
                let model_path = managed_model_path(&spec)?;
                model_readiness_at(&model_path, model, Some(pair))
            }
            TranslationRequest::CustomOpus { path } => {
                model_readiness_at(path, TranslationModel::Opus, None)
            }
            TranslationRequest::CustomNllb { path, pair } => {
                model_readiness_at(path, TranslationModel::Nllb, Some(pair))
            }
        }
    }

    /// Explicitly prepares a managed model without terminal input.
    pub(crate) fn prepare(
        &self,
        mut progress: impl FnMut(SetupProgress),
    ) -> Result<ModelReadiness> {
        match self.resolve_translation_request()? {
            TranslationRequest::ManagedPair { model, pair } => {
                let spec = model_spec(model, pair)?;
                let model_path = managed_model_path(&spec)?;
                if model_path.exists() {
                    return model_readiness_at(&model_path, model, Some(pair));
                }
                progress(SetupProgress {
                    component: "translation_model".into(),
                    stage: "download".into(),
                    message: format!("Downloading and converting {}", spec.hugging_face_id),
                    completed: None,
                    total: None,
                });
                if let Err(error) = prepare_managed_model(&spec, model, pair, |line| {
                    progress(SetupProgress {
                        component: "translation_model".into(),
                        stage: "download".into(),
                        message: line,
                        completed: None,
                        total: None,
                    });
                }) {
                    progress(SetupProgress {
                        component: "translation_model".into(),
                        stage: "failed".into(),
                        message: error.to_string(),
                        completed: None,
                        total: None,
                    });
                    return Err(error);
                }
                progress(SetupProgress {
                    component: "translation_model".into(),
                    stage: "complete".into(),
                    message: format!("Prepared {}", model_path.display()),
                    completed: Some(1),
                    total: Some(1),
                });
                model_readiness_at(&model_path, model, Some(pair))
            }
            TranslationRequest::CustomOpus { path } => {
                model_readiness_at(path, TranslationModel::Opus, None)
            }
            TranslationRequest::CustomNllb { path, pair } => {
                model_readiness_at(path, TranslationModel::Nllb, Some(pair))
            }
        }
    }

    pub(crate) fn open_ready_translator(&self) -> Result<Ct2Translator> {
        Ct2Translator::load_ready_model(self.resolve_translation_request()?)
    }

    pub(crate) fn open_translator_for_cli(&self) -> Result<Ct2Translator> {
        let request = self.resolve_translation_request()?;
        match &request {
            TranslationRequest::ManagedPair { model, pair } => {
                let spec = model_spec(*model, *pair)?;
                prepare_managed_model_interactive(&spec, *model, *pair)?;
            }
            TranslationRequest::CustomOpus { path } => {
                validate_model(path, TranslationModel::Opus, None).with_context(|| {
                    format!("custom translation model {} is not usable", path.display())
                })?;
            }
            TranslationRequest::CustomNllb { path, pair } => {
                validate_model(path, TranslationModel::Nllb, Some(*pair)).with_context(|| {
                    format!("custom translation model {} is not usable", path.display())
                })?;
            }
        }
        Ct2Translator::load_ready_model(request)
    }
}

#[derive(Copy, Clone)]
struct LanguagePair {
    source: TranslationLanguage,
    target: TranslationLanguage,
}

impl LanguagePair {
    fn new(source: TranslationLanguage, target: TranslationLanguage) -> Result<Self> {
        ensure!(source != target, "source and target languages must differ");
        Ok(Self { source, target })
    }
}

enum TranslationRequest<'a> {
    ManagedPair {
        model: TranslationModel,
        pair: LanguagePair,
    },
    CustomOpus {
        path: &'a Path,
    },
    CustomNllb {
        path: &'a Path,
        pair: LanguagePair,
    },
}

pub struct Ct2Translator {
    model: TranslationModel,
    pair: Option<LanguagePair>,
    opus_input_prefix: Option<String>,
    translator: AutoTranslator,
}

impl Ct2Translator {
    fn load_ready_model(request: TranslationRequest<'_>) -> Result<Self> {
        let (model, pair, opus_input_prefix, model_path) = match request {
            TranslationRequest::ManagedPair { model, pair } => {
                let spec = model_spec(model, pair)?;
                let readiness = model_readiness_at(&managed_model_path(&spec)?, model, Some(pair))?;
                ensure!(
                    readiness.ready,
                    "{}",
                    readiness
                        .message
                        .unwrap_or_else(|| "translation model is not ready".into())
                );
                let path = readiness.model_path;
                (model, Some(pair), spec.input_prefix, path)
            }
            TranslationRequest::CustomOpus { path } => {
                validate_model(path, TranslationModel::Opus, None).with_context(|| {
                    format!("custom translation model {} is not usable", path.display())
                })?;
                (TranslationModel::Opus, None, None, path.to_owned())
            }
            TranslationRequest::CustomNllb { path, pair } => {
                validate_model(path, TranslationModel::Nllb, Some(pair)).with_context(|| {
                    format!("custom translation model {} is not usable", path.display())
                })?;
                (TranslationModel::Nllb, Some(pair), None, path.to_owned())
            }
        };

        let config = match model {
            TranslationModel::Opus => Config::default(),
            TranslationModel::Nllb => Config {
                compute_type: ComputeType::INT8,
                num_threads_per_replica: std::thread::available_parallelism()
                    .map(|threads| threads.get())
                    .unwrap_or(0),
                ..Config::default()
            },
        };

        Ok(Self {
            model,
            pair,
            opus_input_prefix,
            translator: CTranslateTranslator::new(&model_path, &config).with_context(|| {
                format!("failed to load translation model {}", model_path.display())
            })?,
        })
    }

    fn translate_opus_units(&self, sources: &[String]) -> Result<Vec<String>> {
        let prefixed = self.opus_input_prefix.as_ref().map(|prefix| {
            sources
                .iter()
                .map(|source| format!("{prefix} {source}"))
                .collect::<Vec<_>>()
        });
        let sources = prefixed.as_deref().unwrap_or(sources);

        Ok(self
            .translator
            .translate_batch(sources, &Default::default(), None)?
            .into_iter()
            .map(|(translation, _)| translation)
            .collect())
    }

    fn translate_nllb_units(&self, units: &[String]) -> Result<Vec<String>> {
        let pair = self
            .pair
            .context("NLLB translation requires source and target languages")?;
        let sources = units
            .iter()
            .map(|unit| format!("{} {unit}", pair.source.nllb_code()))
            .collect::<Vec<_>>();

        let options = TranslationOptions {
            disable_unk: true,
            ..Default::default()
        };
        let target_prefixes = vec![vec![pair.target.nllb_code()]; sources.len()];

        Ok(self
            .translator
            .translate_batch_with_target_prefix(&sources, &target_prefixes, &options, None)?
            .into_iter()
            .map(|(translation, _)| translation)
            .collect())
    }

    pub fn translate_segments(&self, sources: &[String]) -> Result<Vec<String>> {
        translate_segments(
            sources,
            CT2_MAX_TRANSLATION_UNIT_BYTES,
            |units| match self.model {
                TranslationModel::Opus => self.translate_opus_units(units),
                TranslationModel::Nllb => self.translate_nllb_units(units),
            },
        )
    }
}

struct ModelSpec {
    hugging_face_id: String,
    directory: String,
    copy_files: &'static [&'static str],
    input_prefix: Option<String>,
}

fn model_spec(model: TranslationModel, pair: LanguagePair) -> Result<ModelSpec> {
    let spec = match model {
        TranslationModel::Nllb => ModelSpec {
            hugging_face_id: NLLB_MODEL_ID.into(),
            directory: "nllb-200-distilled-600M".into(),
            copy_files: &["tokenizer.json"],
            input_prefix: None,
        },
        TranslationModel::Opus => {
            let source = pair.source.code();
            let target = pair.target.code();
            let Some(opus_model) = opus_pairs::model(source, target) else {
                bail!(
                    "afterprint has no known managed OPUS model for {source} -> {target}; \
                     no download was attempted. Use NLLB, or, if the catalog is incomplete, \
                     omit --translation-source and --translation-target and point \
                     --translation-model-path at a converted OPUS model"
                );
            };
            let directory = format!("opus-mt-{}", opus_model.suffix);

            ModelSpec {
                hugging_face_id: format!("Helsinki-NLP/{directory}"),
                directory,
                copy_files: &["source.spm", "target.spm"],
                input_prefix: opus_model.input_prefix,
            }
        }
    };

    Ok(spec)
}

fn prepare_managed_model(
    spec: &ModelSpec,
    model: TranslationModel,
    pair: LanguagePair,
    mut on_output: impl FnMut(String),
) -> Result<PathBuf> {
    let model_path = managed_model_path(spec)?;
    let models_dir = model_path
        .parent()
        .context("managed model path has no parent")?;

    if model_path.exists() {
        validate_model(&model_path, model, Some(pair))?;
        return Ok(model_path);
    }

    fs::create_dir_all(models_dir).with_context(|| {
        format!(
            "failed to create managed model directory {}",
            models_dir.display()
        )
    })?;

    let download_path = next_partial_model_path(models_dir, &spec.directory)?;
    let mut child = Command::new("ct2-transformers-converter")
        .args(["--model", spec.hugging_face_id.as_str(), "--output_dir"])
        .arg(&download_path)
        .arg("--copy_files")
        .args(spec.copy_files)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(
            "failed to start ct2-transformers-converter; install the Python setup dependencies from README.md",
        )?;
    let stdout = child
        .stdout
        .take()
        .context("model setup stdout was not piped")?;
    let stderr = child
        .stderr
        .take()
        .context("model setup stderr was not piped")?;
    let (sender, receiver) = mpsc::channel();
    let stdout_reader = forward_converter_output("stdout", stdout, sender.clone());
    let stderr_reader = forward_converter_output("stderr", stderr, sender.clone());
    drop(sender);
    for (stream, line) in receiver {
        on_output(format!("{stream}: {line}"));
    }
    stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("model setup stdout reader panicked"))?;
    stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("model setup stderr reader panicked"))?;
    let status = child.wait().context("failed to wait for model setup")?;
    ensure!(
        status.success(),
        "model download/conversion failed with {status}; partial files are at {}",
        download_path.display()
    );
    validate_model(&download_path, model, Some(pair))?;
    fs::rename(&download_path, &model_path).with_context(|| {
        format!(
            "failed to publish model from {} to {}",
            download_path.display(),
            model_path.display()
        )
    })?;

    Ok(model_path)
}

fn next_partial_model_path(models_dir: &Path, model_name: &str) -> Result<PathBuf> {
    loop {
        let attempt = NEXT_MODEL_DOWNLOAD_ATTEMPT.fetch_add(1, Ordering::Relaxed);
        let path = models_dir.join(format!(".{model_name}.{}.{attempt}.partial", process::id()));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
    }
}

fn prepare_managed_model_interactive(
    spec: &ModelSpec,
    model: TranslationModel,
    pair: LanguagePair,
) -> Result<PathBuf> {
    let model_path = managed_model_path(spec)?;
    if model_path.exists() {
        validate_model(&model_path, model, Some(pair))?;
        return Ok(model_path);
    }

    let models_dir = model_path
        .parent()
        .context("managed model path has no parent")?;
    eprintln!(
        "No model for {} -> {} was found at {}.\n\
         Setup requires ct2-transformers-converter and downloads {} from Hugging Face.\n\
         Manual converter arguments:\n  --model {}\n  --output_dir {}\n  --copy_files {}",
        pair.source.code(),
        pair.target.code(),
        model_path.display(),
        spec.hugging_face_id,
        spec.hugging_face_id,
        model_path.display(),
        spec.copy_files.join(" "),
    );
    if !confirm("Try to download and convert this model now?")? {
        bail!("translation model setup declined");
    }

    if !models_dir.exists() {
        if !confirm(&format!("Create {}?", models_dir.display()))? {
            bail!("managed model directory creation declined");
        }
    }

    prepare_managed_model(spec, model, pair, |line| eprintln!("{line}"))
}

fn forward_converter_output<R>(
    stream: &'static str,
    reader: R,
    sender: mpsc::Sender<(String, String)>,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        for line in io::BufReader::new(reader).lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => format!("failed to read model setup output: {error}"),
            };
            if sender.send((stream.into(), line)).is_err() {
                break;
            }
        }
    })
}

fn managed_model_path(spec: &ModelSpec) -> Result<PathBuf> {
    let home =
        env::var_os("HOME").context("HOME is not set; provide --translation-model-path instead")?;
    Ok(PathBuf::from(home)
        .join(".config/afterprint/models")
        .join(&spec.directory))
}

fn model_readiness_at(
    model_path: &Path,
    model: TranslationModel,
    pair: Option<LanguagePair>,
) -> Result<ModelReadiness> {
    let readiness = match validate_model(model_path, model, pair) {
        Ok(()) => ModelReadiness {
            ready: true,
            model_path: model_path.to_owned(),
            message: None,
        },
        Err(error) => ModelReadiness {
            ready: false,
            model_path: model_path.to_owned(),
            message: Some(error.to_string()),
        },
    };
    Ok(readiness)
}

fn validate_model(path: &Path, model: TranslationModel, pair: Option<LanguagePair>) -> Result<()> {
    for file in ["config.json", "model.bin", "shared_vocabulary.json"] {
        ensure!(
            path.join(file).is_file(),
            "missing {file} in {}",
            path.display()
        );
    }

    match model {
        TranslationModel::Opus => {
            for file in ["source.spm", "target.spm"] {
                ensure!(
                    path.join(file).is_file(),
                    "missing {file} in {}",
                    path.display()
                );
            }
        }
        TranslationModel::Nllb => {
            let pair = pair.context("NLLB models require a language pair")?;
            ensure!(
                path.join("tokenizer.json").is_file(),
                "missing tokenizer.json in {}",
                path.display()
            );
            let vocabulary: Vec<String> = serde_json::from_reader(
                fs::File::open(path.join("shared_vocabulary.json"))
                    .context("failed to open NLLB vocabulary")?,
            )
            .context("failed to read NLLB vocabulary")?;
            for language in [pair.source, pair.target] {
                ensure!(
                    vocabulary.iter().any(|token| token == language.nllb_code()),
                    "NLLB model does not contain language {}",
                    language.code()
                );
            }
        }
    }

    Ok(())
}

fn confirm(prompt: &str) -> Result<bool> {
    ensure!(
        io::stdin().is_terminal(),
        "model setup needs an interactive terminal; prepare the model manually or use --translation-model-path"
    );
    eprint!("{prompt} [y/N] ");
    io::stderr().flush().context("failed to write prompt")?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("failed to read setup confirmation")?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_model_readiness_and_preparation_do_not_need_a_tty() {
        let root = std::env::temp_dir().join(format!(
            "afterprint-model-readiness-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let model_path = root.join("opus-model");
        fs::create_dir_all(&model_path).unwrap();
        for file in [
            "config.json",
            "model.bin",
            "shared_vocabulary.json",
            "source.spm",
            "target.spm",
        ] {
            fs::write(model_path.join(file), []).unwrap();
        }
        let config = TranslationConfig {
            model: TranslationModel::Opus,
            source: None,
            target: None,
            model_path: Some(model_path.clone()),
        };

        assert!(config.readiness().unwrap().ready);
        assert!(
            config
                .prepare(|_| panic!("custom model needs no setup"))
                .unwrap()
                .ready
        );

        let invalid = TranslationConfig {
            model_path: Some(root.join("missing-model")),
            ..config
        };
        let readiness = invalid.readiness().unwrap();
        assert!(!readiness.ready);
        assert!(readiness.message.unwrap().contains("missing config.json"));
        assert!(
            !invalid
                .prepare(|_| panic!("invalid custom model needs no setup"))
                .unwrap()
                .ready
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_model_setup_attempt_can_be_retried_in_the_same_process() {
        let root = std::env::temp_dir().join(format!(
            "afterprint-model-retry-{}-{}",
            process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let failed_attempt = next_partial_model_path(&root, "opus-mt-en-ro").unwrap();
        fs::write(&failed_attempt, "partial output").unwrap();
        let retry = next_partial_model_path(&root, "opus-mt-en-ro").unwrap();

        assert_ne!(failed_attempt, retry);
        assert!(!retry.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
