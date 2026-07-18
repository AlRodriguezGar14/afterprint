use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use tesseract::{PageSegMode, Tesseract};
use walkdir::WalkDir;

use crate::enums::OcrMode;

/// Run OCR on one image with PSM 3 (auto page segmentation), which preserves
/// the paragraph/line spacing of the original page.
pub fn ocr_page(path: &str, language: &str) -> Result<Tesseract> {
    let mut api = Tesseract::new(None, Some(language))
        .context("failed to initialize Tesseract OCR")?
        .set_image(path)
        .context("failed to load image")?;

    api.set_page_seg_mode(PageSegMode::PsmAuto);
    api.recognize().context("OCR recognition failed")
}

pub fn collect_recognized_text(api: &mut Tesseract, mode: OcrMode) -> Option<String> {
    match mode {
        OcrMode::Text => api.get_text().ok(),
        OcrMode::Hocr => api.get_hocr_text(0).ok(),
        OcrMode::Tsv => api.get_tsv_text(0).ok(),
    }
}

pub fn ocr_pages_in_dir(
    image_folder: &Utf8PathBuf,
    ocr_language: &str,
) -> impl Iterator<Item = Tesseract> {
    WalkDir::new(image_folder)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let path = entry.into_path();

            println!(">>>>{}<<<<<", path.display());

            let path_str = path.to_str()?;
            let Ok(api) = ocr_page(path_str, ocr_language) else {
                return None;
            };

            Some(api)
        })
}

pub fn write_text_to_file(text: &[u8], output_path: &Utf8PathBuf) -> Result<()> {
    std::fs::write(output_path, text)
        .with_context(|| format!("failed to write text to {output_path}"))
}

pub fn write_page(
    out: &Utf8PathBuf,
    index: usize,
    mode: OcrMode,
    text: &str,
) -> Result<Utf8PathBuf> {
    let filename = format!("{:04}.{}", index + 1, mode.extension());
    let output_path = out.join(filename);
    write_text_to_file(text.as_bytes(), &output_path)?;
    Ok(output_path)
}
