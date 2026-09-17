use anyhow::{Result, ensure};
use serde::Serialize;

use crate::document::{Block, Document, Page, PageStatus};

/// Explains the accepted approximation recorded with every debug segment.
pub const PAGE_MAPPING_LIMITATION: &str = "Translated text is split at deterministic proportional word boundaries; page breaks are approximate in v1.";

/// Inclusive one-based physical page span retained by a logical segment.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PageSpan {
    /// First physical page in the segment.
    pub start: usize,
    /// Last physical page in the segment.
    pub end: usize,
}

/// Filtered source text and the physical pages it came from.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Segment {
    /// Stable document-local segment identifier.
    pub id: usize,
    /// Inclusive one-based source page span.
    pub page_span: PageSpan,
    /// Normalized source text sent to translation.
    pub source_text: String,
    /// Known v1 limitation for mapping translated text back to pages.
    pub mapping_limitation: &'static str,
    #[serde(skip)]
    page_parts: Vec<PagePart>,
    #[serde(skip)]
    last_block: Block,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PagePart {
    page: usize,
    source_text: String,
}

/// Builds deterministic logical segments from filtered page content.
///
/// A segment can cross only an adjacent successful page boundary. The
/// boundary is joined when the final and initial OCR blocks have compatible
/// geometry and at least one continuation signal.
pub fn build_segments(document: &Document) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut previous_page: Option<&Page> = None;

    for page in &document.pages {
        let paragraphs = page_paragraphs(page);
        if paragraphs.is_empty() {
            previous_page = None;
            continue;
        }

        for (paragraph_index, paragraph) in paragraphs.into_iter().enumerate() {
            let target = (paragraph_index == 0)
                .then(|| {
                    continuation_target(&segments, previous_page, page, &paragraph.first_block)
                })
                .flatten();

            if let Some(target) = target {
                let segment = &mut segments[target];
                segment.page_span.end = page.index + 1;
                segment.source_text = join_text(&segment.source_text, &paragraph.source_text);
                segment.page_parts.push(PagePart {
                    page: page.index,
                    source_text: paragraph.source_text,
                });
                segment.last_block = paragraph.last_block;
            } else {
                segments.push(Segment {
                    id: 0,
                    page_span: PageSpan {
                        start: page.index + 1,
                        end: page.index + 1,
                    },
                    source_text: paragraph.source_text.clone(),
                    mapping_limitation: PAGE_MAPPING_LIMITATION,
                    page_parts: vec![PagePart {
                        page: page.index,
                        source_text: paragraph.source_text,
                    }],
                    last_block: paragraph.last_block,
                });
            }
        }
        previous_page = Some(page);
    }

    for (id, segment) in segments.iter_mut().enumerate() {
        segment.id = id + 1;
    }
    segments
}

fn continuation_target(
    segments: &[Segment],
    previous_page: Option<&Page>,
    next: &Page,
    first_block: &Block,
) -> Option<usize> {
    let previous = previous_page?;
    let mut index = segments.len();
    while index > 0 {
        let segment = &segments[index - 1];
        if segment.page_span.end != next.index {
            break;
        }
        if is_footer_like(previous, &segment.last_block) {
            index -= 1;
            continue;
        }
        return can_continue(previous, &segment.last_block, next, first_block).then_some(index - 1);
    }
    None
}

/// Maps each translated logical segment back to its original physical pages.
/// The returned vector is indexed by zero-based physical page index.
pub fn map_translations_to_pages(
    segments: &[Segment],
    translations: &[String],
    page_count: usize,
) -> Result<Vec<String>> {
    ensure!(
        segments.len() == translations.len(),
        "received {} translations for {} segments",
        translations.len(),
        segments.len(),
    );

    let mut pages = vec![Vec::new(); page_count];
    for (segment, translation) in segments.iter().zip(translations) {
        for (part, translated_part) in segment
            .page_parts
            .iter()
            .zip(split_translation(translation, &segment.page_parts))
        {
            if !translated_part.is_empty() {
                ensure!(
                    part.page < page_count,
                    "segment {} references page {} outside document",
                    segment.id,
                    part.page + 1,
                );
                pages[part.page].push(translated_part);
            }
        }
    }

    Ok(pages.into_iter().map(|parts| parts.join("\n\n")).collect())
}

struct PageParagraph {
    source_text: String,
    first_block: Block,
    last_block: Block,
}

/// Uses contiguous OCR block numbers as paragraph candidates because P0-1
/// retains line geometry but not Tesseract's paragraph number.
fn page_paragraphs(page: &Page) -> Vec<PageParagraph> {
    let mut paragraphs = Vec::<PageParagraph>::new();
    for block in &page.filtered_blocks {
        if block.text.trim().is_empty() {
            continue;
        }
        let same_block = paragraphs
            .last()
            .is_some_and(|paragraph| paragraph.last_block.block_num == block.block_num);
        if same_block {
            let paragraph = paragraphs
                .last_mut()
                .expect("same_block requires a paragraph");
            paragraph.source_text = join_text(&paragraph.source_text, &block.text);
            paragraph.last_block = block.clone();
        } else {
            paragraphs.push(PageParagraph {
                source_text: block.text.trim().to_string(),
                first_block: block.clone(),
                last_block: block.clone(),
            });
        }
    }
    paragraphs
}

fn join_text(left: &str, right: &str) -> String {
    [left.trim(), right.trim()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn can_continue(previous: &Page, last: &Block, next: &Page, first: &Block) -> bool {
    if !matches!(&previous.status, PageStatus::OcrSucceeded)
        || !matches!(&next.status, PageStatus::OcrSucceeded)
    {
        return false;
    }

    compatible_geometry(previous, last, next, first)
        && (missing_terminal_punctuation(&last.text)
            || continuation_compatible_start(&first.text)
            || trailing_continuation_punctuation(&last.text))
}

fn is_footer_like(page: &Page, block: &Block) -> bool {
    let height = page.page_height.max(1) as f32;
    let bottom = (block.coordinates.top + block.coordinates.height).max(0) as f32 / height;
    bottom >= 0.88 && block.text.chars().count() <= 32 && block.confidence < 70.0
}

fn compatible_geometry(previous: &Page, last: &Block, next: &Page, first: &Block) -> bool {
    let previous_width = previous.page_width.max(1) as f32;
    let next_width = next.page_width.max(1) as f32;
    let previous_left = last.coordinates.left as f32 / previous_width;
    let next_left = first.coordinates.left as f32 / next_width;
    let heights_compatible = last.coordinates.height <= 0 || first.coordinates.height <= 0 || {
        let ratio = last.coordinates.height as f32 / first.coordinates.height as f32;
        (0.4..=2.5).contains(&ratio)
    };
    (previous_left - next_left).abs() <= 0.12 && heights_compatible
}

fn missing_terminal_punctuation(text: &str) -> bool {
    let text = text.trim_end();
    let text = text.trim_end_matches(['"', '\'', '”', '’', '»', ')', ']', '}']);
    !text.is_empty()
        && text
            .chars()
            .last()
            .is_some_and(|character| !matches!(character, '.' | '!' | '?' | '。' | '！' | '？'))
}

fn trailing_continuation_punctuation(text: &str) -> bool {
    text.trim_end().chars().last().is_some_and(|character| {
        matches!(
            character,
            ',' | ';' | ':' | '-' | '–' | '—' | '(' | '“' | '‘'
        )
    })
}

fn continuation_compatible_start(text: &str) -> bool {
    text.trim_start().chars().next().is_some_and(|character| {
        character.is_lowercase()
            || matches!(character, ',' | ';' | ':' | ')' | ']' | '}' | '—' | '–')
    })
}

fn split_translation(translation: &str, parts: &[PagePart]) -> Vec<String> {
    if parts.len() <= 1 || translation.trim().is_empty() {
        return vec![translation.trim().to_string(); parts.len()];
    }

    let source_total = parts
        .iter()
        .map(|part| part.source_text.chars().count().max(1))
        .sum::<usize>();
    let translation_chars = translation.chars().count();
    let mut output = Vec::with_capacity(parts.len());
    let mut start = 0;
    let mut start_chars = 0;
    let mut source_offset = 0;

    for part in &parts[..parts.len() - 1] {
        source_offset += part.source_text.chars().count().max(1);
        let desired = translation_chars * source_offset / source_total;
        let relative_cut =
            nearest_whitespace_boundary(&translation[start..], desired.saturating_sub(start_chars));
        let cut = start + relative_cut;
        output.push(translation[start..cut].trim().to_string());
        start = cut;
        start_chars = translation[..start].chars().count();
    }
    output.push(translation[start..].trim().to_string());
    output
}

fn nearest_whitespace_boundary(text: &str, desired_chars: usize) -> usize {
    let boundaries = text
        .char_indices()
        .enumerate()
        .filter_map(|(char_index, (byte_index, character))| {
            character
                .is_whitespace()
                .then_some((byte_index, char_index))
        })
        .collect::<Vec<_>>();
    let Some((boundary, _)) = boundaries.into_iter().min_by_key(|(_, char_index)| {
        (*char_index as isize - desired_chars as isize).unsigned_abs()
    }) else {
        return text.len();
    };
    boundary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Coordinates, PageStatus};
    use crate::enums::OcrMode;

    fn page(index: usize, text: &str, left: i32) -> Page {
        page_with_blocks(index, &[(1, text, left)])
    }

    fn page_with_blocks(index: usize, entries: &[(u32, &str, i32)]) -> Page {
        let blocks = entries
            .iter()
            .enumerate()
            .map(|(line_num, (block_num, text, left))| Block {
                block_num: *block_num,
                line_num: line_num as u32 + 1,
                text: (*text).to_string(),
                coordinates: Coordinates {
                    left: *left,
                    top: 400 + line_num as i32 * 30,
                    width: 400,
                    height: 20,
                },
                confidence: 90.0,
            })
            .collect::<Vec<_>>();
        Page {
            index,
            path: format!("{index}.png"),
            status: PageStatus::OcrSucceeded,
            raw_text: blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            raw_output: blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            raw_output_mode: OcrMode::Text,
            filtered_blocks: blocks.clone(),
            blocks,
            page_width: 1000,
            page_height: 1000,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn joins_a_page_spanning_paragraph() {
        let document = Document {
            pages: vec![
                page(0, "The paragraph continues", 100),
                page(1, "on page two.", 100),
            ],
        };

        let segments = build_segments(&document);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].id, 1);
        assert_eq!(segments[0].page_span, PageSpan { start: 1, end: 2 });
        assert_eq!(
            segments[0].source_text,
            "The paragraph continues on page two."
        );
    }

    #[test]
    fn does_not_join_completed_paragraphs() {
        let document = Document {
            pages: vec![
                page(0, "The first paragraph ends.", 100),
                page(1, "A new paragraph starts.", 100),
            ],
        };

        let segments = build_segments(&document);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].page_span, PageSpan { start: 1, end: 1 });
        assert_eq!(segments[1].page_span, PageSpan { start: 2, end: 2 });
    }

    #[test]
    fn keeps_same_page_paragraph_candidates_separate_before_joining_the_last_one() {
        let document = Document {
            pages: vec![
                page_with_blocks(
                    0,
                    &[
                        (1, "The first paragraph ends.", 100),
                        (2, "The second continues", 100),
                    ],
                ),
                page(1, "on page two.", 100),
            ],
        };

        let segments = build_segments(&document);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source_text, "The first paragraph ends.");
        assert_eq!(segments[0].page_span, PageSpan { start: 1, end: 1 });
        assert_eq!(segments[1].source_text, "The second continues on page two.");
        assert_eq!(segments[1].page_span, PageSpan { start: 1, end: 2 });
    }

    #[test]
    fn skips_a_low_confidence_footer_when_finding_the_body_continuation() {
        let mut previous =
            page_with_blocks(0, &[(1, "The paragraph continues", 100), (2, "PE îi", 100)]);
        previous.filtered_blocks[1].coordinates.top = 900;
        previous.filtered_blocks[1].confidence = 35.0;
        let document = Document {
            pages: vec![previous, page(1, "on page two.", 100)],
        };

        let segments = build_segments(&document);

        assert_eq!(segments.len(), 2);
        let body = segments
            .iter()
            .find(|segment| segment.source_text.contains("The paragraph"))
            .unwrap();
        assert_eq!(body.page_span, PageSpan { start: 1, end: 2 });
        assert!(segments.iter().any(|segment| {
            segment.source_text == "PE îi" && segment.page_span == PageSpan { start: 1, end: 1 }
        }));

        let translations = segments
            .iter()
            .map(|segment| segment.source_text.clone())
            .collect::<Vec<_>>();
        let mapped = map_translations_to_pages(&segments, &translations, 2).unwrap();
        let mapped_text = mapped.join(" ");
        for token in [
            "The",
            "paragraph",
            "continues",
            "on",
            "page",
            "two.",
            "PE",
            "îi",
        ] {
            assert!(mapped_text.split_whitespace().any(|word| word == token));
        }
    }

    #[test]
    fn failed_pages_block_cross_page_reconstruction() {
        let failed = Page {
            index: 1,
            path: "1.png".to_string(),
            status: PageStatus::OcrFailed {
                error: "OCR failed".to_string(),
            },
            raw_text: String::new(),
            raw_output: String::new(),
            raw_output_mode: OcrMode::Text,
            filtered_blocks: Vec::new(),
            blocks: Vec::new(),
            page_width: 0,
            page_height: 0,
            warnings: Vec::new(),
        };
        let document = Document {
            pages: vec![
                page(0, "The paragraph continues", 100),
                failed,
                page(2, "on page three.", 100),
            ],
        };

        let segments = build_segments(&document);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].page_span, PageSpan { start: 1, end: 1 });
        assert_eq!(segments[1].page_span, PageSpan { start: 3, end: 3 });
    }

    #[test]
    fn maps_cross_page_translation_once_at_a_word_boundary() {
        let document = Document {
            pages: vec![
                page(0, "A paragraph continues", 100),
                page(1, "on page two.", 100),
            ],
        };
        let segments = build_segments(&document);

        let mapped = map_translations_to_pages(
            &segments,
            &["Une phrase continue sur la deuxième page.".to_string()],
            2,
        )
        .unwrap();

        assert_eq!(
            mapped.join(" ").split_whitespace().collect::<Vec<_>>(),
            vec![
                "Une",
                "phrase",
                "continue",
                "sur",
                "la",
                "deuxième",
                "page."
            ]
        );
        assert!(!mapped[0].is_empty());
        assert!(!mapped[1].is_empty());
    }
}
