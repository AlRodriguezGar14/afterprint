use crate::document::{Block, Coordinates, Page, PageStatus};
use crate::enums::OcrMode;
use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use std::path::{Path, PathBuf};
use tesseract::{PageSegMode, Tesseract};
use walkdir::WalkDir;

// Tesseract TSV order: level, page_num, block_num, par_num, line_num,
// word_num, left, top, width, height, conf, text.
const TSV_HEADER: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";
const TSV_COLUMN_COUNT: usize = 12;
const TSV_LEVEL_COLUMN: usize = 0;
const TSV_BLOCK_NUMBER_COLUMN: usize = 2;
const TSV_PARAGRAPH_NUMBER_COLUMN: usize = 3;
const TSV_LINE_NUMBER_COLUMN: usize = 4;
const TSV_LEFT_COLUMN: usize = 6;
const TSV_TOP_COLUMN: usize = 7;
const TSV_WIDTH_COLUMN: usize = 8;
const TSV_HEIGHT_COLUMN: usize = 9;
const TSV_CONFIDENCE_COLUMN: usize = 10;
const TSV_TEXT_COLUMN: usize = 11;
const TSV_PAGE_LEVEL: u32 = 1;
const TSV_WORD_LEVEL: u32 = 5;

/// Runs Tesseract recognition for `path` in `language` and returns the
/// recognized handle. The caller reads the outputs; no files are written.
pub fn ocr_page(path: &str, language: &str) -> Result<Tesseract> {
    let mut api = Tesseract::new(None, Some(language))
        .context("failed to initialize Tesseract OCR")?
        .set_image(path)
        .context("failed to load image")?;

    api.set_page_seg_mode(PageSegMode::PsmAuto);
    api.recognize().context("OCR recognition failed")
}

/// Yields one [`Page`] per naturally sorted input file in `image_folder`.
/// Each page receives its zero-based physical index before OCR; failures remain
/// in the sequence, and no files are written.
pub fn ocr_pages_in_dir(
    image_folder: &Utf8PathBuf,
    ocr_language: &str,
    mode: OcrMode,
) -> impl Iterator<Item = Page> {
    image_paths(image_folder)
        .into_iter()
        .enumerate()
        .map(move |(index, path)| {
            let display_path = path.display().to_string();
            let Some(path_str) = path.to_str() else {
                return failed_page(index, display_path, "input path is not valid UTF-8");
            };

            let api = match ocr_page(path_str, ocr_language) {
                Ok(api) => api,
                Err(error) => return failed_page(index, display_path, &error.to_string()),
            };

            collect_ocr_page(index, display_path, api, mode)
        })
}

fn image_paths(image_folder: &Utf8PathBuf) -> Vec<PathBuf> {
    WalkDir::new(image_folder)
        .sort_by(|a, b| {
            natord::compare(
                &a.file_name().to_string_lossy(),
                &b.file_name().to_string_lossy(),
            )
        })
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file() && is_supported_image(entry.path()))
        .map(|entry| entry.into_path())
        .collect()
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "bmp"
                    | "gif"
                    | "jpeg"
                    | "jpg"
                    | "pbm"
                    | "pgm"
                    | "png"
                    | "ppm"
                    | "tif"
                    | "tiff"
                    | "webp"
            )
        })
}

fn collect_ocr_page(index: usize, path: String, mut api: Tesseract, mode: OcrMode) -> Page {
    let raw_text = match api.get_text() {
        Ok(text) => text,
        Err(error) => {
            return failed_page(index, path, &format!("failed to read OCR text: {error}"));
        }
    };
    let mut warnings = Vec::new();

    let tsv = match api.get_tsv_text(0) {
        Ok(tsv) => Some(with_tsv_header(&tsv)),
        Err(error) => {
            warnings.push(format!("failed to read OCR TSV layout: {error}"));
            None
        }
    };
    let (page_width, page_height, blocks) = tsv.as_deref().map(parse_tsv).unwrap_or_default();

    let mut output_mode = mode;
    let raw_output = match mode {
        OcrMode::Text => raw_text.clone(),
        OcrMode::Hocr => match api.get_hocr_text(0) {
            Ok(output) => output,
            Err(error) => {
                output_mode = OcrMode::Text;
                warnings.push(format!(
                    "failed to read OCR hOCR output: {error}; writing raw text as .txt"
                ));
                raw_text.clone()
            }
        },
        OcrMode::Tsv => tsv.unwrap_or_else(|| {
            output_mode = OcrMode::Text;
            warnings.push("TSV output unavailable; writing raw text as .txt".to_string());
            raw_text.clone()
        }),
    };

    Page {
        index,
        path,
        status: PageStatus::OcrSucceeded,
        raw_text,
        raw_output,
        raw_output_mode: output_mode,
        filtered_blocks: blocks.clone(),
        blocks,
        page_width,
        page_height,
        warnings,
    }
}

fn with_tsv_header(tsv: &str) -> String {
    let mut output = String::with_capacity(TSV_HEADER.len() + tsv.len());
    output.push_str(TSV_HEADER);
    output.push_str(tsv);
    output
}

fn failed_page(index: usize, path: String, error: &str) -> Page {
    Page {
        index,
        path,
        status: PageStatus::OcrFailed {
            error: error.to_string(),
        },
        raw_text: String::new(),
        raw_output: String::new(),
        raw_output_mode: OcrMode::Text,
        blocks: Vec::new(),
        filtered_blocks: Vec::new(),
        page_width: 0,
        page_height: 0,
        warnings: Vec::new(),
    }
}

/// Returns page dimensions and line-level [`Block`]s from Tesseract TSV input
/// beginning with its canonical 12-column header.
/// The `level` field is hierarchy depth: 1 page, 2 block, 3 paragraph,
/// 4 line, and 5 word. Page rows supply dimensions; word rows are grouped by
/// block/paragraph/line and aggregated into blocks.
/// Rows missing columns or malformed required fields are skipped; malformed confidence is treated as `0.0`.
fn parse_tsv(tsv: &str) -> (u32, u32, Vec<Block>) {
    let mut page_width = 0;
    let mut page_height = 0;
    let mut lines = std::collections::BTreeMap::<(u32, u32, u32), ParsedLine>::new();

    // Skip the canonical TSV header; the remaining lines are data rows.
    for row in tsv.lines().skip(1) {
        let fields = row.splitn(TSV_COLUMN_COUNT, '\t').collect::<Vec<_>>();
        if fields.len() < TSV_COLUMN_COUNT {
            continue;
        }
        let level = fields[TSV_LEVEL_COLUMN].parse::<u32>().ok();
        if level == Some(TSV_PAGE_LEVEL) {
            page_width = fields[TSV_WIDTH_COLUMN].parse().unwrap_or(0);
            page_height = fields[TSV_HEIGHT_COLUMN].parse().unwrap_or(0);
        }
        if level != Some(TSV_WORD_LEVEL) || fields[TSV_TEXT_COLUMN].trim().is_empty() {
            continue;
        }

        let Some(block_num) = fields[TSV_BLOCK_NUMBER_COLUMN].parse::<u32>().ok() else {
            continue;
        };
        let Some(paragraph_num) = fields[TSV_PARAGRAPH_NUMBER_COLUMN].parse::<u32>().ok() else {
            continue;
        };
        let Some(line_num) = fields[TSV_LINE_NUMBER_COLUMN].parse::<u32>().ok() else {
            continue;
        };
        let Some(left) = fields[TSV_LEFT_COLUMN].parse::<i32>().ok() else {
            continue;
        };
        let Some(top) = fields[TSV_TOP_COLUMN].parse::<i32>().ok() else {
            continue;
        };
        let Some(width) = fields[TSV_WIDTH_COLUMN].parse::<i32>().ok() else {
            continue;
        };
        let Some(height) = fields[TSV_HEIGHT_COLUMN].parse::<i32>().ok() else {
            continue;
        };
        let confidence = fields[TSV_CONFIDENCE_COLUMN].parse::<f32>().unwrap_or(0.0);
        let entry = lines
            .entry((block_num, paragraph_num, line_num))
            .or_insert_with(|| ParsedLine {
                text: String::new(),
                coordinates: Coordinates {
                    left,
                    top,
                    width,
                    height,
                },
                confidence_total: 0.0,
                confidence_count: 0,
            });
        if !entry.text.is_empty() {
            entry.text.push(' ');
        }
        entry.text.push_str(fields[TSV_TEXT_COLUMN].trim());
        entry.coordinates.left = entry.coordinates.left.min(left);
        entry.coordinates.top = entry.coordinates.top.min(top);
        let right = (entry.coordinates.left + entry.coordinates.width).max(left + width);
        let bottom = (entry.coordinates.top + entry.coordinates.height).max(top + height);
        entry.coordinates.width = right - entry.coordinates.left;
        entry.coordinates.height = bottom - entry.coordinates.top;
        if confidence >= 0.0 {
            entry.confidence_total += confidence;
            entry.confidence_count += 1;
        }
    }

    let blocks = lines
        .into_iter()
        .map(|((block_num, _, line_num), line)| Block {
            block_num,
            line_num,
            text: line.text,
            coordinates: line.coordinates,
            confidence: if line.confidence_count == 0 {
                0.0
            } else {
                line.confidence_total / line.confidence_count as f32
            },
        })
        .collect();
    (page_width, page_height, blocks)
}

#[derive(Debug)]
struct ParsedLine {
    text: String,
    coordinates: Coordinates,
    confidence_total: f32,
    confidence_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tsv_coordinates_and_average_confidence() {
        let tsv = concat!(
            "1\t1\t0\t0\t0\t0\t0\t0\t1000\t2000\t-1\t\n",
            "5\t1\t2\t1\t3\t1\t10\t20\t30\t40\t80\tHello\n",
            "5\t1\t2\t1\t3\t2\t45\t20\t50\t40\t60\tworld\n",
        );
        let tsv = format!("{TSV_HEADER}{tsv}");

        let (width, height, blocks) = parse_tsv(&tsv);

        assert_eq!((width, height), (1000, 2000));
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "Hello world");
        assert_eq!(blocks[0].block_num, 2);
        assert_eq!(blocks[0].line_num, 3);
        assert_eq!(
            blocks[0].coordinates,
            Coordinates {
                left: 10,
                top: 20,
                width: 85,
                height: 40,
            }
        );
        assert_eq!(blocks[0].confidence, 70.0);
    }

    #[test]
    fn adds_the_canonical_header_at_the_tesseract_boundary() {
        let tsv = with_tsv_header("1\t1\t0\t0\t0\t0\t0\t0\t1000\t2000\t-1\t\n");

        assert!(tsv.starts_with(TSV_HEADER));
        assert_eq!(parse_tsv(&tsv).0, 1000);
    }

    #[test]
    fn failed_ocr_returns_a_page_with_its_physical_index() {
        let page = failed_page(1, "middle.png".to_string(), "OCR failed");

        assert_eq!(page.index, 1);
        assert!(page.raw_text.is_empty());
        assert!(matches!(page.status, PageStatus::OcrFailed { .. }));
    }

    #[test]
    fn only_supported_image_extensions_are_ocr_inputs() {
        assert!(is_supported_image(Path::new("page.JPG")));
        assert!(is_supported_image(Path::new("page.png")));
        assert!(!is_supported_image(Path::new(".DS_Store")));
        assert!(!is_supported_image(Path::new("notes.txt")));
    }

    #[test]
    fn unsupported_files_do_not_consume_the_first_page_index() {
        let folder = std::env::temp_dir().join(format!(
            "afterprint-ocr-inputs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(".DS_Store"), []).unwrap();
        std::fs::write(folder.join("002.jpg"), []).unwrap();
        std::fs::write(folder.join("001.jpg"), []).unwrap();
        let folder = Utf8PathBuf::from_path_buf(folder).unwrap();

        let indexed_paths = image_paths(&folder)
            .into_iter()
            .enumerate()
            .collect::<Vec<_>>();

        assert_eq!(indexed_paths.len(), 2);
        assert_eq!(indexed_paths[0].0, 0);
        assert_eq!(indexed_paths[0].1.file_name().unwrap(), "001.jpg");
        assert_eq!(indexed_paths[1].0, 1);
        assert_eq!(indexed_paths[1].1.file_name().unwrap(), "002.jpg");
        std::fs::remove_dir_all(folder).unwrap();
    }
}
