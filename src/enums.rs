use clap::ValueEnum;

#[derive(Copy, Clone, Debug, ValueEnum)]
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

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum TranslationModel {
    Opus,
    Nllb,
}
