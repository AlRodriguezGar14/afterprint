use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use printpdf::{
    Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt, TextItem,
};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::document::{Document, Page};

const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;
const MARGIN_MM: f32 = 20.0;
const MAX_FONT_SIZE: f32 = 14.0;
const MIN_FONT_SIZE: f32 = 2.0;
const LINE_HEIGHT_FACTOR: f32 = 1.25;

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum PdfContent {
    Filtered,
    Raw,
}

impl PdfContent {
    pub fn text(self, page: &Page) -> String {
        match self {
            Self::Filtered => page.filtered_text(),
            Self::Raw => page.raw_text.clone(),
        }
    }
}

#[derive(Clone)]
struct LoadedFont {
    id: printpdf::FontId,
    parsed: ParsedFont,
}

/// Writes one fixed A4 page for every physical page in `document`.
/// The page text is supplied by the caller so source and translated PDFs share
/// exactly the same page-indexed renderer.
pub fn write_pdf(
    out: &camino::Utf8Path,
    filename: &str,
    document: &Document,
    page_texts: &[String],
    title: &str,
    author: &str,
    language: &str,
) -> Result<()> {
    ensure!(
        page_texts.len() == document.pages.len(),
        "PDF text has {} pages for {} input pages",
        page_texts.len(),
        document.pages.len(),
    );

    let mut pdf = PdfDocument::new(title);
    pdf.metadata.info.author = author.to_string();
    pdf.metadata.info.subject = language.to_string();
    let fonts = load_fonts(&mut pdf)?;
    let pages = page_texts
        .iter()
        .map(|text| render_page(text, &fonts))
        .collect::<Result<Vec<_>>>()?;
    pdf.with_pages(pages);

    let bytes = save_pdf(pdf, language)?;
    let path = out.join(filename);
    std::fs::write(&path, bytes).with_context(|| format!("failed to write PDF to {path}"))?;
    println!("wrote {filename}");
    Ok(())
}

fn load_fonts(pdf: &mut PdfDocument) -> Result<Vec<LoadedFont>> {
    [
        (&["NotoSans-Regular.ttf", "Arial Unicode.ttf"][..], 0),
        (
            &[
                "NotoSansArabic-Regular.ttf",
                "SFArabic.ttf",
                "Arial Unicode.ttf",
            ][..],
            0,
        ),
        (
            &[
                "NotoSansCJK-Regular.ttc",
                "Hiragino Sans GB.ttc",
                "Arial Unicode.ttf",
            ][..],
            0,
        ),
    ]
    .into_iter()
    .map(|(filenames, index)| {
        let path = find_font(filenames).ok_or_else(|| {
            anyhow::anyhow!(
                "required PDF font was not found; tried {}",
                filenames.join(", ")
            )
        })?;
        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read PDF font {}", path.display()))?;
        let parsed = ParsedFont::from_bytes(&bytes, index, &mut Vec::new())
            .ok_or_else(|| anyhow::anyhow!("failed to parse PDF font {}", path.display()))?;
        let id = pdf.add_font(&parsed);
        Ok(LoadedFont { id, parsed })
    })
    .collect()
}

fn find_font(filenames: &[&str]) -> Option<PathBuf> {
    [
        "/usr/share/fonts",
        "/Library/Fonts",
        "/System/Library/Fonts",
    ]
    .into_iter()
    .filter(|root| Path::new(root).exists())
    .flat_map(|root| {
        WalkDir::new(root)
            .into_iter()
            .filter_map(|entry| entry.ok())
    })
    .find(|entry| {
        entry.path().is_file()
            && filenames
                .iter()
                .any(|filename| entry.file_name().to_string_lossy() == *filename)
    })
    .map(|entry| entry.into_path())
}

fn render_page(text: &str, fonts: &[LoadedFont]) -> Result<PdfPage> {
    let font_size = fitted_font_size(text);
    let max_chars = max_chars_per_line(font_size);
    let lines = wrap_text(text, max_chars);
    let line_height = font_size * LINE_HEIGHT_FACTOR;
    let mut ops = vec![
        Op::StartTextSection,
        Op::SetTextCursor {
            pos: Point::new(Mm(MARGIN_MM), Mm(PAGE_HEIGHT_MM - MARGIN_MM)),
        },
        Op::SetLineHeight {
            lh: Pt(line_height),
        },
    ];

    for line in lines {
        append_font_runs(&mut ops, &line, fonts, font_size)?;
        ops.push(Op::AddLineBreak);
    }
    ops.push(Op::EndTextSection);
    Ok(PdfPage::new(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), ops))
}

fn append_font_runs(
    ops: &mut Vec<Op>,
    line: &str,
    fonts: &[LoadedFont],
    font_size: f32,
) -> Result<()> {
    if line.is_empty() {
        return Ok(());
    }

    let mut runs = Vec::<(usize, String)>::new();
    for character in line.chars() {
        let font_index = fonts
            .iter()
            .position(|font| font.parsed.lookup_glyph_index(character as u32).is_some())
            .unwrap_or(0);
        if runs.last().is_some_and(|(index, _)| *index == font_index) {
            runs.last_mut().expect("last run exists").1.push(character);
        } else {
            runs.push((font_index, character.to_string()));
        }
    }

    for (font_index, text) in runs {
        let Some(font) = fonts.get(font_index) else {
            bail!("PDF font fallback index {font_index} is unavailable")
        };
        ops.push(Op::SetFont {
            font: PdfFontHandle::External(font.id.clone()),
            size: Pt(font_size),
        });
        ops.push(Op::ShowText {
            items: vec![TextItem::Text(text)],
        });
    }
    Ok(())
}

fn save_pdf(pdf: PdfDocument, language: &str) -> Result<Vec<u8>> {
    let options = PdfSaveOptions::default();
    let mut warnings = Vec::new();
    let mut document = pdf.to_lopdf_document(&options, &mut warnings);
    document
        .catalog_mut()
        .context("generated PDF has no catalog")?
        .set("Lang", lopdf::Object::string_literal(language));
    let mut bytes = Vec::new();
    document
        .save_to(&mut bytes)
        .context("failed to serialize PDF")?;
    Ok(bytes)
}

fn fitted_font_size(text: &str) -> f32 {
    let mut size = MAX_FONT_SIZE;
    while size > MIN_FONT_SIZE {
        if wrap_text(text, max_chars_per_line(size)).len() <= available_lines(size) {
            return size;
        }
        size -= 0.5;
    }
    MIN_FONT_SIZE
}

fn available_lines(font_size: f32) -> usize {
    let height_pt = (PAGE_HEIGHT_MM - MARGIN_MM * 2.0) * 72.0 / 25.4;
    (height_pt / (font_size * LINE_HEIGHT_FACTOR))
        .floor()
        .max(1.0) as usize
}

fn max_chars_per_line(font_size: f32) -> usize {
    ((PAGE_WIDTH_MM - MARGIN_MM * 2.0) * 72.0 / 25.4 / font_size)
        .floor()
        .max(1.0) as usize
}

fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for source_line in text.lines() {
        if source_line.trim().is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut line = String::new();
        for word in source_line.split_whitespace() {
            if word.chars().count() > max_chars {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                for chunk in word.chars().collect::<Vec<_>>().chunks(max_chars) {
                    lines.push(chunk.iter().collect());
                }
                continue;
            }
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if candidate.chars().count() > max_chars {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Block, Page, PageStatus};
    use crate::enums::OcrMode;

    fn page(index: usize, raw: &str, filtered: &str) -> Page {
        Page {
            index,
            path: format!("{index}.png"),
            status: PageStatus::OcrSucceeded,
            raw_text: raw.to_string(),
            raw_output: raw.to_string(),
            raw_output_mode: OcrMode::Text,
            blocks: Vec::new(),
            filtered_blocks: if filtered.is_empty() {
                Vec::new()
            } else {
                vec![Block::new(1, 1, filtered, 0, 0, 1, 1)]
            },
            page_width: 100,
            page_height: 100,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn selects_filtered_or_raw_page_content() {
        let page = page(0, "header\nbody", "body");

        assert_eq!(PdfContent::Filtered.text(&page), "body");
        assert_eq!(PdfContent::Raw.text(&page), "header\nbody");
    }

    #[test]
    fn wraps_long_words_and_preserves_explicit_blank_lines() {
        assert_eq!(
            wrap_text("one two\n\nabcdefgh", 4),
            vec![
                "one".to_string(),
                "two".to_string(),
                String::new(),
                "abcd".to_string(),
                "efgh".to_string()
            ]
        );
    }

    #[test]
    fn preserves_unicode_text_while_wrapping() {
        let text = "Latin Ελληνικά Кириллица العربية 日本語";

        assert_eq!(wrap_text(text, 100), vec![text.to_string()]);
    }

    #[test]
    fn fitting_reduces_font_size_for_dense_pages_without_dropping_text() {
        let text = (0..5000).map(|_| "word").collect::<Vec<_>>().join(" ");
        let size = fitted_font_size(&text);

        assert!(size < MAX_FONT_SIZE);
        assert!(!wrap_text(&text, max_chars_per_line(size)).is_empty());
    }

    #[test]
    fn a4_page_constants_are_fixed() {
        assert_eq!((PAGE_WIDTH_MM, PAGE_HEIGHT_MM), (210.0, 297.0));
        assert_eq!(available_lines(MAX_FONT_SIZE), 41);
    }

    #[test]
    fn failed_page_renders_as_a_blank_a4_page() {
        let page = render_page("", &[]).unwrap();

        assert_eq!(page.media_box.width.0, 595.27563);
        assert_eq!(page.media_box.height.0, 841.88983);
        assert_eq!(page.ops.len(), 5);
    }

    #[test]
    fn metadata_save_adds_language_without_images() {
        let mut pdf = PdfDocument::new("Book");
        pdf.metadata.info.author = "Author".to_string();
        pdf.metadata.info.subject = "ja".to_string();
        pdf.with_pages(vec![PdfPage::new(
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            Vec::new(),
        )]);

        let bytes = save_pdf(pdf, "ja").unwrap();
        assert!(
            bytes
                .windows(b"/Lang(ja)".len())
                .any(|window| window == b"/Lang(ja)")
        );
        assert!(
            !bytes
                .windows(b"/Subtype /Image".len())
                .any(|window| window == b"/Subtype /Image")
        );
    }
}
