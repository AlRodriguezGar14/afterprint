use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use ct2rs::{
    ComputeType, Config, TranslationOptions, Translator as CTranslateTranslator,
    tokenizers::auto::Tokenizer as AutoTokenizer,
};
use std::{
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{self, Command},
};

use crate::enums::{TranslationLanguage, TranslationModel};

use super::{opus_pairs, text::translate_paragraphs};

const CT2_MAX_TRANSLATION_UNIT_CHARS: usize = 800;
const NLLB_MODEL_ID: &str = "facebook/nllb-200-distilled-600M";

type AutoTranslator = CTranslateTranslator<AutoTokenizer>;

#[derive(Args, Debug, Default)]
pub struct TranslationArgs {
    /// Enable translation with OPUS or NLLB.
    #[arg(long, value_enum)]
    translation_model: Option<TranslationModel>,

    /// Source language for a managed model, or for a custom NLLB model.
    #[arg(long, value_enum, requires = "translation_model")]
    translation_source: Option<TranslationLanguage>,

    /// Target language for a managed model, or for a custom NLLB model.
    #[arg(long, value_enum, requires = "translation_model")]
    translation_target: Option<TranslationLanguage>,

    /// Use an existing converted model instead of a managed model.
    #[arg(long, requires = "translation_model")]
    translation_model_path: Option<PathBuf>,
}

impl TranslationArgs {
    /// Resolves the CLI options, then loads or interactively prepares the model.
    pub fn open(&self) -> Result<Option<Ct2Translator>> {
        self.request()?.map(Ct2Translator::open).transpose()
    }

    fn request(&self) -> Result<Option<TranslationRequest<'_>>> {
        let Some(model) = self.translation_model else {
            return Ok(None);
        };

        let request = match (
            model,
            self.translation_model_path.as_deref(),
            self.translation_source,
            self.translation_target,
        ) {
            (TranslationModel::Opus, Some(path), None, None) => {
                TranslationRequest::CustomOpus { path }
            }
            (TranslationModel::Opus, Some(_), _, _) => bail!(
                "a custom OPUS model defines its own language direction; omit \
                 --translation-source and --translation-target"
            ),
            (TranslationModel::Nllb, Some(path), Some(source), Some(target)) => {
                TranslationRequest::CustomNllb {
                    path,
                    pair: LanguagePair::new(source, target)?,
                }
            }
            (_, None, Some(source), Some(target)) => TranslationRequest::ManagedPair {
                model,
                pair: LanguagePair::new(source, target)?,
            },
            (TranslationModel::Nllb, Some(_), _, _) => {
                bail!("a custom NLLB model requires --translation-source and --translation-target")
            }
            (_, None, _, _) => bail!(
                "--translation-source and --translation-target are required for a managed model"
            ),
        };

        Ok(Some(request))
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
    fn open(request: TranslationRequest<'_>) -> Result<Self> {
        let (model, pair, opus_input_prefix, model_path) = match request {
            TranslationRequest::ManagedPair { model, pair } => {
                let spec = model_spec(model, pair)?;
                let path = prepare_managed_model(&spec, model, pair)?;
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

    pub fn translate_page(&self, source: &str) -> Result<String> {
        translate_paragraphs(source, CT2_MAX_TRANSLATION_UNIT_CHARS, |units| {
            match self.model {
                TranslationModel::Opus => self.translate_opus_units(units),
                TranslationModel::Nllb => self.translate_nllb_units(units),
            }
        })
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
) -> Result<PathBuf> {
    let home =
        env::var_os("HOME").context("HOME is not set; provide --translation-model-path instead")?;
    let models_dir = PathBuf::from(home).join(".config/afterprint/models");
    let model_path = models_dir.join(&spec.directory);

    if model_path.exists() {
        validate_model(&model_path, model, Some(pair))?;
        return Ok(model_path);
    }

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
        fs::create_dir_all(&models_dir).with_context(|| {
            format!(
                "failed to create managed model directory {}",
                models_dir.display()
            )
        })?;
    }

    let download_path = models_dir.join(format!(".{}.{}.partial", spec.directory, process::id()));
    ensure!(
        !download_path.exists(),
        "temporary model path already exists: {}",
        download_path.display()
    );
    let status = Command::new("ct2-transformers-converter")
        .args(["--model", spec.hugging_face_id.as_str(), "--output_dir"])
        .arg(&download_path)
        .arg("--copy_files")
        .args(spec.copy_files)
        .status()
        .context(
            "failed to start ct2-transformers-converter; install the Python setup dependencies from README.md",
        )?;
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
    print!("{prompt} [y/N] ");
    io::stdout().flush().context("failed to write prompt")?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("failed to read setup confirmation")?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
