use anyhow::{Context, Result};
use ct2rs::{
    ComputeType, Config, TranslationOptions, Translator as CTranslateTranslator,
    tokenizers::auto::Tokenizer as AutoTokenizer,
};
use std::path::{Path, PathBuf};

use crate::enums::TranslationModel;

use super::text::translate_paragraphs;

// TODO: enhance the user input so Opus can have multiple language combinations
const OPUS_MODEL_PATH: &str = "models/opus-mt-ROMANCE-en";
const NLLB_MODEL_PATH: &str = "models/nllb-200-distilled-600M";
const CT2_MAX_TRANSLATION_UNIT_CHARS: usize = 800;

type AutoTranslator = CTranslateTranslator<AutoTokenizer>;

pub struct Ct2Translator {
    model: TranslationModel,
    translator: AutoTranslator,
}

impl Ct2Translator {
    pub fn new(model: TranslationModel) -> Result<Self> {
        let (model_path, config) = match model {
            TranslationModel::Opus => (model_path(OPUS_MODEL_PATH), Config::default()),
            TranslationModel::Nllb => (
                model_path(NLLB_MODEL_PATH),
                Config {
                    compute_type: ComputeType::INT8,
                    num_threads_per_replica: std::thread::available_parallelism()
                        .map(|threads| threads.get())
                        .unwrap_or(0),
                    ..Config::default()
                },
            ),
        };

        Ok(Self {
            model,
            translator: CTranslateTranslator::new(&model_path, &config)
                .context("failed to load translation model")?,
        })
    }

    fn translate_opus_units(&self, sources: &[String]) -> Result<Vec<String>> {
        Ok(self
            .translator
            .translate_batch(sources, &Default::default(), None)?
            .into_iter()
            .map(|(translation, _)| translation)
            .collect())
    }

    fn translate_nllb_units(&self, units: &[String]) -> Result<Vec<String>> {
        let sources = units
            .iter()
            // TODO: Handle different languages by user input
            .map(|unit| format!("ron_Latn {unit}"))
            .collect::<Vec<_>>();

        let options = TranslationOptions {
            disable_unk: true,
            ..Default::default()
        };

        // TODO: Handle different languages by user input
        let target_prefixes = vec![vec!["eng_Latn"]; sources.len()];
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

fn model_path(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path)
}
