use clap::{ValueEnum, builder::PossibleValue};

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum OcrMode {
    Text,
    Hocr,
    Tsv,
}

impl OcrMode {
    pub fn extension(&self) -> &str {
        match self {
            OcrMode::Text => "txt",
            OcrMode::Hocr => "html",
            OcrMode::Tsv => "tsv",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum TranslationModel {
    Opus,
    Nllb,
}

/// Standardized CLI language: `code` indexes OPUS, while `nllb_code` is the
/// tokenizer token required by NLLB.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TranslationLanguage {
    code: &'static str,
    nllb_code: &'static str,
}

impl TranslationLanguage {
    const fn new(code: &'static str, nllb_code: &'static str) -> Self {
        Self { code, nllb_code }
    }

    pub fn code(self) -> &'static str {
        self.code
    }

    pub fn nllb_code(self) -> &'static str {
        self.nllb_code
    }
}

impl ValueEnum for TranslationLanguage {
    fn value_variants<'a>() -> &'a [Self] {
        TRANSLATION_LANGUAGES
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.code))
    }
}

const TRANSLATION_LANGUAGES: &[TranslationLanguage] = &[
    TranslationLanguage::new("af", "afr_Latn"),
    TranslationLanguage::new("sq", "als_Latn"),
    TranslationLanguage::new("am", "amh_Ethi"),
    TranslationLanguage::new("ar", "arb_Arab"),
    TranslationLanguage::new("hy", "hye_Armn"),
    TranslationLanguage::new("as", "asm_Beng"),
    TranslationLanguage::new("az", "azj_Latn"),
    TranslationLanguage::new("eu", "eus_Latn"),
    TranslationLanguage::new("be", "bel_Cyrl"),
    TranslationLanguage::new("bn", "ben_Beng"),
    TranslationLanguage::new("bho", "bho_Deva"),
    TranslationLanguage::new("bs", "bos_Latn"),
    TranslationLanguage::new("bg", "bul_Cyrl"),
    TranslationLanguage::new("my", "mya_Mymr"),
    TranslationLanguage::new("ca", "cat_Latn"),
    TranslationLanguage::new("ceb", "ceb_Latn"),
    TranslationLanguage::new("zh-hans", "zho_Hans"),
    TranslationLanguage::new("zh-hant", "zho_Hant"),
    TranslationLanguage::new("hr", "hrv_Latn"),
    TranslationLanguage::new("cs", "ces_Latn"),
    TranslationLanguage::new("da", "dan_Latn"),
    TranslationLanguage::new("nl", "nld_Latn"),
    TranslationLanguage::new("en", "eng_Latn"),
    TranslationLanguage::new("et", "est_Latn"),
    TranslationLanguage::new("fo", "fao_Latn"),
    TranslationLanguage::new("fi", "fin_Latn"),
    TranslationLanguage::new("fr", "fra_Latn"),
    TranslationLanguage::new("gl", "glg_Latn"),
    TranslationLanguage::new("ka", "kat_Geor"),
    TranslationLanguage::new("de", "deu_Latn"),
    TranslationLanguage::new("el", "ell_Grek"),
    TranslationLanguage::new("gu", "guj_Gujr"),
    TranslationLanguage::new("ha", "hau_Latn"),
    TranslationLanguage::new("he", "heb_Hebr"),
    TranslationLanguage::new("hi", "hin_Deva"),
    TranslationLanguage::new("hu", "hun_Latn"),
    TranslationLanguage::new("is", "isl_Latn"),
    TranslationLanguage::new("ig", "ibo_Latn"),
    TranslationLanguage::new("id", "ind_Latn"),
    TranslationLanguage::new("ga", "gle_Latn"),
    TranslationLanguage::new("it", "ita_Latn"),
    TranslationLanguage::new("ja", "jpn_Jpan"),
    TranslationLanguage::new("jv", "jav_Latn"),
    TranslationLanguage::new("kn", "kan_Knda"),
    TranslationLanguage::new("kk", "kaz_Cyrl"),
    TranslationLanguage::new("km", "khm_Khmr"),
    TranslationLanguage::new("ko", "kor_Hang"),
    TranslationLanguage::new("ku", "kmr_Latn"),
    TranslationLanguage::new("ky", "kir_Cyrl"),
    TranslationLanguage::new("lo", "lao_Laoo"),
    TranslationLanguage::new("lv", "lvs_Latn"),
    TranslationLanguage::new("lt", "lit_Latn"),
    TranslationLanguage::new("lb", "ltz_Latn"),
    TranslationLanguage::new("mk", "mkd_Cyrl"),
    TranslationLanguage::new("ml", "mal_Mlym"),
    TranslationLanguage::new("mt", "mlt_Latn"),
    TranslationLanguage::new("mr", "mar_Deva"),
    TranslationLanguage::new("mn", "khk_Cyrl"),
    TranslationLanguage::new("ne", "npi_Deva"),
    TranslationLanguage::new("nb", "nob_Latn"),
    TranslationLanguage::new("nn", "nno_Latn"),
    TranslationLanguage::new("or", "ory_Orya"),
    TranslationLanguage::new("ps", "pbt_Arab"),
    TranslationLanguage::new("fa", "pes_Arab"),
    TranslationLanguage::new("pl", "pol_Latn"),
    TranslationLanguage::new("pt", "por_Latn"),
    TranslationLanguage::new("pa", "pan_Guru"),
    TranslationLanguage::new("ro", "ron_Latn"),
    TranslationLanguage::new("ru", "rus_Cyrl"),
    TranslationLanguage::new("gd", "gla_Latn"),
    TranslationLanguage::new("sr", "srp_Cyrl"),
    TranslationLanguage::new("sd", "snd_Arab"),
    TranslationLanguage::new("si", "sin_Sinh"),
    TranslationLanguage::new("sk", "slk_Latn"),
    TranslationLanguage::new("sl", "slv_Latn"),
    TranslationLanguage::new("so", "som_Latn"),
    TranslationLanguage::new("es", "spa_Latn"),
    TranslationLanguage::new("su", "sun_Latn"),
    TranslationLanguage::new("sw", "swh_Latn"),
    TranslationLanguage::new("sv", "swe_Latn"),
    TranslationLanguage::new("tl", "tgl_Latn"),
    TranslationLanguage::new("ta", "tam_Taml"),
    TranslationLanguage::new("te", "tel_Telu"),
    TranslationLanguage::new("th", "tha_Thai"),
    TranslationLanguage::new("tr", "tur_Latn"),
    TranslationLanguage::new("tk", "tuk_Latn"),
    TranslationLanguage::new("uk", "ukr_Cyrl"),
    TranslationLanguage::new("ur", "urd_Arab"),
    TranslationLanguage::new("uz", "uzn_Latn"),
    TranslationLanguage::new("vi", "vie_Latn"),
    TranslationLanguage::new("cy", "cym_Latn"),
    TranslationLanguage::new("yo", "yor_Latn"),
    TranslationLanguage::new("zu", "zul_Latn"),
];
