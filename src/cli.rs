use crate::{
    RunConfig,
    enums::{OcrMode, TRANSLATION_LANGUAGES, TranslationLanguage, TranslationModel},
    pdf::PdfContent,
    translation::TranslationConfig,
};
use anyhow::Context;
use camino::Utf8PathBuf;
use clap::{
    ArgMatches, Parser, ValueEnum,
    builder::{PossibleValue, PossibleValuesParser},
    parser::ValueSource,
};
use std::path::PathBuf;

#[derive(clap::Args, Debug, Default)]
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
    fn translation_config(&self) -> anyhow::Result<Option<TranslationConfig>> {
        let Some(model) = self.translation_model else {
            return Ok(None);
        };
        let config = TranslationConfig {
            model,
            source: self.translation_source,
            target: self.translation_target,
            model_path: self.translation_model_path.clone(),
        };
        config.validate()?;
        Ok(Some(config))
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
/// Supported machine-readable CLI event output.
pub enum EventFormat {
    Jsonl,
}

#[derive(Parser, Debug)]
/// Clap adapter for the current terminal interface.
#[command(version, about, long_about = None)]
pub struct Args {
    pub image_folder: Option<Utf8PathBuf>,

    #[arg(short, long)]
    pub out: Option<Utf8PathBuf>,

    /// Open the desktop GUI instead of starting a run.
    #[arg(long, conflicts_with = "cli")]
    pub gui: bool,

    /// Run headlessly using the CLI options.
    #[arg(long, conflicts_with = "gui")]
    pub cli: bool,

    #[arg(short, long, default_value = "Untitled")]
    pub title: String,

    #[arg(short, long, default_value = "Unknown")]
    pub author: String,

    #[arg(short, long, value_parser= PossibleValuesParser::new(ISO_639_1_CODES), default_value = "en")]
    pub language: String,

    #[arg(
        long,
        value_parser = PossibleValuesParser::new(TESSERACT_LANGUAGES),
        default_value = "eng"
    )]
    pub ocr_language: String,

    #[arg(long, value_enum, default_value = "text")]
    pub ocr_mode: OcrMode,

    #[command(flatten)]
    pub translation: TranslationArgs,

    #[arg(long)]
    pub pdf: bool,

    #[arg(long, value_enum, default_value = "filtered")]
    pub pdf_content: PdfContent,

    /// Emit a versioned machine-readable event stream.
    #[arg(long, value_enum)]
    pub events: Option<EventFormat>,
}

impl Args {
    /// Parses command-line arguments once using Clap.
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }

    /// Converts parsed CLI values to the same configuration used by libraries.
    pub fn run_config(&self) -> anyhow::Result<RunConfig> {
        Ok(RunConfig {
            input: self
                .image_folder
                .clone()
                .context("a scan folder is required")?
                .into_std_path_buf(),
            output: self
                .out
                .clone()
                .context("an output folder is required; pass --out")?
                .into_std_path_buf(),
            ocr_language: self.ocr_language.clone(),
            ocr_mode: self.ocr_mode,
            title: self.title.clone(),
            author: self.author.clone(),
            language: self.language.clone(),
            pdf: self.pdf,
            pdf_content: self.pdf_content,
            translation: self.translation.translation_config()?,
        })
    }

    /// Accepts GUI folder/language prefills and rejects unsupported CLI options.
    pub fn validate_gui_options(matches: &ArgMatches) -> anyhow::Result<()> {
        let unsupported = [
            ("title", "--title"),
            ("author", "--author"),
            ("language", "--language"),
            ("ocr_mode", "--ocr-mode"),
            ("translation_model", "--translation-model"),
            ("translation_source", "--translation-source"),
            ("translation_target", "--translation-target"),
            ("translation_model_path", "--translation-model-path"),
            ("pdf", "--pdf"),
            ("pdf_content", "--pdf-content"),
        ]
        .into_iter()
        .filter(|(id, _)| matches.value_source(id) == Some(ValueSource::CommandLine))
        .map(|(_, flag)| flag)
        .collect::<Vec<_>>();

        if unsupported.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "the GUI supports scan folder, --out, and --ocr-language only; use --cli for {}",
                unsupported.join(", ")
            )
        }
    }
}

impl ValueEnum for OcrMode {
    fn value_variants<'a>() -> &'a [Self] {
        OCR_MODES
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(match self {
            Self::Text => "text",
            Self::Hocr => "hocr",
            Self::Tsv => "tsv",
        }))
    }
}

impl ValueEnum for PdfContent {
    fn value_variants<'a>() -> &'a [Self] {
        PDF_CONTENTS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(match self {
            Self::Filtered => "filtered",
            Self::Raw => "raw",
        }))
    }
}

impl ValueEnum for TranslationModel {
    fn value_variants<'a>() -> &'a [Self] {
        TRANSLATION_MODELS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(match self {
            Self::Opus => "opus",
            Self::Nllb => "nllb",
        }))
    }
}

const OCR_MODES: &[OcrMode] = &[OcrMode::Text, OcrMode::Hocr, OcrMode::Tsv];
const PDF_CONTENTS: &[PdfContent] = &[PdfContent::Filtered, PdfContent::Raw];
const TRANSLATION_MODELS: &[TranslationModel] = &[TranslationModel::Opus, TranslationModel::Nllb];

impl ValueEnum for TranslationLanguage {
    fn value_variants<'a>() -> &'a [Self] {
        TRANSLATION_LANGUAGES
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn parses_pdf_options_and_rejects_removed_flags() {
        let args = Args::try_parse_from([
            "afterprint",
            "scans",
            "--out",
            "out",
            "--pdf",
            "--pdf-content",
            "raw",
        ])
        .unwrap();

        assert!(args.pdf);
        assert_eq!(args.pdf_content, PdfContent::Raw);
        assert!(Args::try_parse_from(["afterprint", "scans", "--out", "out", "--no-pdf"]).is_err());
    }

    #[test]
    fn parses_jsonl_event_output() {
        let args =
            Args::try_parse_from(["afterprint", "scans", "--out", "out", "--events", "jsonl"])
                .unwrap();

        assert_eq!(args.events, Some(EventFormat::Jsonl));
    }

    #[test]
    fn gui_mode_accepts_partial_configuration_without_starting_a_run() {
        let args = Args::try_parse_from(["afterprint", "--gui", "--ocr-language", "fra"]).unwrap();

        assert!(args.gui);
        assert_eq!(args.image_folder, None);
        assert_eq!(args.out, None);
        assert_eq!(args.ocr_language, "fra");
    }

    #[test]
    fn mistyped_romanian_language_code_suggests_ron() {
        let error = Args::try_parse_from(["afterprint", "--gui", "--ocr-language", "rom"])
            .unwrap_err()
            .to_string();

        assert!(error.contains("similar value exists: 'ron'"));
    }

    #[test]
    fn gui_and_cli_modes_cannot_be_combined() {
        assert!(Args::try_parse_from(["afterprint", "--gui", "--cli"]).is_err());
    }

    #[test]
    fn gui_rejects_advanced_options_but_accepts_mvp_prefills() {
        let matches = Args::command()
            .try_get_matches_from(["afterprint", "--gui", "--pdf"])
            .unwrap();
        assert!(
            Args::validate_gui_options(&matches)
                .unwrap_err()
                .to_string()
                .contains("--pdf")
        );

        let matches = Args::command()
            .try_get_matches_from([
                "afterprint",
                "--gui",
                "scans",
                "--out",
                "out",
                "--ocr-language",
                "fra",
            ])
            .unwrap();
        assert!(Args::validate_gui_options(&matches).is_ok());
    }
}

const ISO_639_1_CODES: &[&str] = &[
    "ab", "aa", "af", "ak", "sq", "am", "ar", "an", "hy", "as", "av", "ae", "ay", "az", "bm", "ba",
    "eu", "be", "bn", "bh", "bi", "bs", "br", "bg", "my", "ca", "km", "ch", "ce", "ny", "zh", "cu",
    "cv", "kw", "co", "cr", "hr", "cs", "da", "dv", "nl", "dz", "en", "eo", "et", "ee", "fo", "fj",
    "fi", "fr", "ff", "gd", "gl", "lg", "ka", "de", "ki", "el", "kl", "gn", "gu", "ht", "ha", "he",
    "hz", "hi", "ho", "hu", "is", "io", "ig", "id", "ia", "ie", "iu", "ik", "ga", "it", "ja", "jv",
    "kn", "kr", "ks", "kk", "rw", "kv", "kg", "ko", "kj", "ku", "ky", "lo", "la", "lv", "lb", "li",
    "ln", "lt", "lu", "mk", "mg", "ms", "ml", "mt", "gv", "mi", "mr", "mh", "ro", "mn", "na", "nv",
    "nd", "ng", "ne", "se", "no", "nb", "nn", "ii", "oc", "oj", "or", "om", "os", "pi", "pa", "ps",
    "fa", "pl", "pt", "qu", "rm", "rn", "ru", "sm", "sg", "sa", "sc", "sr", "sn", "sd", "si", "sk",
    "sl", "so", "st", "nr", "es", "su", "sw", "ss", "sv", "tl", "ty", "tg", "ta", "tt", "te", "th",
    "bo", "ti", "to", "ts", "tn", "tr", "tk", "tw", "ug", "uk", "ur", "uz", "ve", "vi", "vo", "wa",
    "cy", "fy", "wo", "xh", "yi", "yo", "za", "zu",
];

pub(crate) const TESSERACT_LANGUAGES: &[&str] = &[
    "afr", "amh", "ara", "asm", "aze", "aze_cyrl", "bel", "ben", "bod", "bos", "brew", "bul",
    "cat", "ceb", "ces", "chi_sim", "chi_tra", "chr", "cos", "cym", "dan", "dan_frak", "deu",
    "deu_frak", "deu_latf", "dzo", "ell", "eng", "enm", "epo", "equ", "est", "eus", "fao", "fas",
    "fil", "fin", "fra", "frk", "frm", "fry", "gla", "gle", "glg", "grc", "guj", "hat", "heb",
    "hin", "hrv", "hun", "hye", "iku", "ind", "isl", "ita", "ita_old", "jav", "jpn", "kan", "kat",
    "kat_old", "kaz", "khm", "kir", "kmr", "kor", "lao", "lat", "lav", "lit", "ltz", "mal", "mar",
    "mkd", "mlt", "mon", "mri", "msa", "mya", "nep", "nld", "nor", "oci", "ori", "osd", "pan",
    "pol", "por", "pus", "que", "ron", "rus", "san", "sin", "slk", "slk_frak", "slv", "snd", "spa",
    "spa_old", "sqi", "srp", "srp_latn", "sun", "swa", "swe", "syr", "tam", "tat", "tel", "tgk",
    "tgl", "tha", "tir", "ton", "tur", "uig", "ukr", "urd", "uzb", "uzb_cyrl", "vie", "yid", "yod",
];
