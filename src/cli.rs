use crate::enums::OcrMode;
use camino::Utf8PathBuf;
use clap::{Parser, builder::PossibleValuesParser};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    pub image_folder: Utf8PathBuf,

    #[arg(short, long)]
    pub out: Utf8PathBuf,

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

    #[arg(long, default_value = "a4")]
    pub page_size: String,

    #[arg(long, value_enum, default_value = "text")]
    pub ocr_mode: OcrMode,

    #[arg(long)]
    pub force_text: bool,

    #[arg(long)]
    pub no_pdf: bool,
}

impl Args {
    pub fn parse() -> Self {
        <Self as Parser>::parse()
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

const TESSERACT_LANGUAGES: &[&str] = &[
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
