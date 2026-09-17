use serde::Serialize;
use std::collections::HashMap;

use crate::document::{Block, Coordinates, Document, Page};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    RepeatedTopLine,
    RepeatedBottomLine,
    FixedPositionPageNumber,
    NormalizedNumericVariant,
    SeparatorLikeFooter,
    IsolatedShortFooter,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
/// Identifies one OCR block removed from filtered output and why it was removed.
pub struct Exclusion {
    /// One-based page number used in reports; source `Page::index` is zero-based.
    pub page: usize,
    /// Original OCR text retained for auditing the decision.
    pub text: String,
    /// Original OCR geometry retained for auditing the decision.
    pub coordinates: Coordinates,
    /// Deterministic signal that justified removing the block.
    pub reason: ExclusionReason,
}

#[derive(Clone, Debug)]
/// Comparison-only view derived from one OCR block before filtering.
struct Occurrence {
    /// Internal zero-based `Page::index` used for cross-page comparisons.
    page: usize,
    /// Whitespace-normalized text used for exact repeated-text matching.
    key: String,
    /// Text with each digit run collapsed, allowing numeric formatting variants to match.
    normalized_key: String,
    /// Optional parsed numeric candidate; it is evidence, not proof of a page number.
    page_number: Option<u32>,
    /// Normalized horizontal position relative to the page width.
    x: f32,
    /// Normalized vertical position of the block's top edge relative to page height.
    top: f32,
    /// Normalized vertical position of the block's bottom edge relative to page height.
    bottom: f32,
}

/// Applies deterministic furniture heuristics, mutating only `filtered_blocks`.
/// Raw blocks and OCR text remain unchanged; returned exclusion pages are one-based.
pub fn filter_document(document: &mut Document) -> Vec<Exclusion> {
    let occurrences = document
        .pages
        .iter()
        .flat_map(|page| {
            page.blocks.iter().enumerate().map(|(block_index, block)| {
                (
                    Occurrence {
                        page: page.index,
                        key: normalize_text(&block.text),
                        normalized_key: normalize_digits(&block.text),
                        page_number: parse_page_number(&block.text),
                        x: position_x(page, block),
                        top: position_top(page, block),
                        bottom: position_bottom(page, block),
                    },
                    block_index,
                )
            })
        })
        .collect::<Vec<_>>();

    let mut exact = HashMap::<String, Vec<Occurrence>>::new();
    let mut normalized = HashMap::<String, Vec<Occurrence>>::new();
    for (occurrence, _) in &occurrences {
        exact
            .entry(occurrence.key.clone())
            .or_default()
            .push(occurrence.clone());
        normalized
            .entry(occurrence.normalized_key.clone())
            .or_default()
            .push(occurrence.clone());
    }
    let page_numbers = occurrences
        .iter()
        .filter(|(occurrence, _)| occurrence.page_number.is_some())
        .map(|(occurrence, _)| occurrence.clone())
        .collect::<Vec<_>>();

    let mut exclusions = Vec::new();
    for page in &mut document.pages {
        let mut keep = vec![true; page.blocks.len()];
        for (block_index, block) in page.blocks.iter().enumerate() {
            let occurrence = occurrences
                .iter()
                .find(|(occurrence, index)| *index == block_index && occurrence.page == page.index)
                .map(|(occurrence, _)| occurrence);
            let Some(occurrence) = occurrence else {
                continue;
            };

            let exact_repeated = repeated_at_position(exact.get(&occurrence.key), occurrence);
            let normalized_repeated =
                repeated_at_position(normalized.get(&occurrence.normalized_key), occurrence);
            let similar_repeated = similar_repeated_at_position(&occurrences, occurrence);
            let top = occurrence.top <= 0.18;
            let bottom = occurrence.bottom >= 0.82;
            let numeric = is_page_number(&block.text);
            let separator = is_separator_like(&block.text);
            let isolated = block_index >= page.blocks.len().saturating_sub(2);
            let short_footer = block.text.chars().count() <= 80;
            let explicit_page_marker = has_explicit_page_marker(&block.text);
            let sequential_page_number = strong_sequential_page_number(&page_numbers, occurrence);
            let identified_numeric = !numeric || explicit_page_marker || sequential_page_number;

            let reason = if numeric && bottom && (explicit_page_marker || sequential_page_number) {
                Some(ExclusionReason::FixedPositionPageNumber)
            } else if separator && bottom && isolated && identified_numeric {
                Some(ExclusionReason::SeparatorLikeFooter)
            } else if bottom
                && isolated
                && short_footer
                && !numeric
                && (exact_repeated || normalized_repeated)
            {
                Some(ExclusionReason::IsolatedShortFooter)
            } else if (exact_repeated || similar_repeated) && top && identified_numeric {
                Some(ExclusionReason::RepeatedTopLine)
            } else if (exact_repeated || similar_repeated) && bottom && identified_numeric {
                Some(ExclusionReason::RepeatedBottomLine)
            } else if (normalized_repeated || similar_repeated)
                && (top || bottom)
                && identified_numeric
            {
                Some(ExclusionReason::NormalizedNumericVariant)
            } else {
                None
            };

            if let Some(reason) = reason {
                keep[block_index] = false;
                exclusions.push(Exclusion {
                    page: page.index + 1,
                    text: block.text.clone(),
                    coordinates: block.coordinates.clone(),
                    reason,
                });
            }
        }
        page.filtered_blocks = page
            .blocks
            .iter()
            .enumerate()
            .filter(|(index, _)| keep[*index])
            .map(|(_, block)| block.clone())
            .collect();
    }

    exclusions
}

/// Reports whether the supplied key group has an occurrence on another page at a comparable normalized position.
fn repeated_at_position(group: Option<&Vec<Occurrence>>, current: &Occurrence) -> bool {
    let Some(group) = group else {
        return false;
    };

    group.iter().any(|other| {
        other.page != current.page
            && (other.x - current.x).abs() <= 0.12
            && (other.top - current.top).abs() <= 0.08
    })
}

/// Reports a conservative noisy-text match on another page at a similar relative position.
fn similar_repeated_at_position(occurrences: &[(Occurrence, usize)], current: &Occurrence) -> bool {
    let current_tokens = furniture_tokens(&current.key);
    if current_tokens.len() < 3 {
        return false;
    }

    // OCR noise is allowed, but each line must mostly overlap the other.
    occurrences.iter().any(|(other, _)| {
        other.page != current.page
            && (other.x - current.x).abs() <= 0.12
            && (other.top - current.top).abs() <= 0.08
            && {
                let other_tokens = furniture_tokens(&other.key);
                let shared_tokens = other_tokens
                    .iter()
                    .filter(|token| current_tokens.contains(token))
                    .count();
                shared_tokens * 4 >= current_tokens.len() * 3
                    && shared_tokens * 4 >= other_tokens.len() * 3
            }
    })
}

/// Returns the block's left position normalized by page width.
fn position_x(page: &Page, block: &Block) -> f32 {
    let width = page.page_width.max(1) as f32;
    block.coordinates.left as f32 / width
}

/// Returns the block's top position normalized by the effective page height.
fn position_top(page: &Page, block: &Block) -> f32 {
    let height = page_height(page);
    block.coordinates.top.max(0) as f32 / height
}

/// Returns the block's bottom position normalized by the effective page height.
fn position_bottom(page: &Page, block: &Block) -> f32 {
    let height = page_height(page);
    (block.coordinates.top + block.coordinates.height).max(0) as f32 / height
}

/// Uses the recorded page height, or the greatest non-negative block bottom, with `1` as fallback.
fn page_height(page: &Page) -> f32 {
    page.page_height.max(
        page.blocks
            .iter()
            .map(|block| (block.coordinates.top + block.coordinates.height).max(0) as u32)
            .max()
            .unwrap_or(1),
    ) as f32
}

/// Normalizes whitespace and case for text comparison; it never changes output text.
fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Collapses each run of ASCII digits to `#` after text normalization for comparison keys.
fn normalize_digits(text: &str) -> String {
    let mut normalized = String::new();
    let mut in_digits = false;
    for character in normalize_text(text).chars() {
        if character.is_ascii_digit() {
            if !in_digits {
                normalized.push('#');
                in_digits = true;
            }
        } else {
            normalized.push(character);
            in_digits = false;
        }
    }
    normalized
}

/// Tokenizes furniture text by replacing punctuation and other non-alphanumeric characters with separators while preserving alphanumeric Unicode characters exactly.
fn furniture_tokens(text: &str) -> Vec<String> {
    normalize_text(text)
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Returns whether normalized text contains a standalone `page` or `of` marker.
fn has_explicit_page_marker(text: &str) -> bool {
    normalize_text(text)
        .split_whitespace()
        .any(|word| matches!(word, "page" | "of"))
}

/// Extracts the first contiguous ASCII digit run as a candidate page number, not proof.
fn parse_page_number(text: &str) -> Option<u32> {
    let digits = text
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

/// Requires another page with the same number-to-page offset and stable relative position.
fn strong_sequential_page_number(group: &[Occurrence], current: &Occurrence) -> bool {
    let Some(current_number) = current.page_number else {
        return false;
    };
    let offset = current_number as i64 - current.page as i64;
    group.iter().any(|other| {
        other.page != current.page
            && other
                .page_number
                .is_some_and(|number| number as i64 - other.page as i64 == offset)
            && (other.x - current.x).abs() <= 0.12
            && (other.top - current.top).abs() <= 0.08
    })
}

/// Accepts short digit-bearing strings made only of page-number punctuation or `page`/`of`.
fn is_page_number(text: &str) -> bool {
    let text = text.trim().to_lowercase();
    if text.is_empty() || text.chars().count() > 24 || !text.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }

    text.split_whitespace().all(|word| {
        word.chars().all(|character| {
            character.is_ascii_digit()
                || matches!(
                    character,
                    '.' | '-' | '–' | '—' | '(' | ')' | '[' | ']' | '#'
                )
        }) || matches!(word, "page" | "of")
    })
}

/// Returns whether at least half of a three-or-more-character string is separator-like.
fn is_separator_like(text: &str) -> bool {
    let characters = text.trim().chars().collect::<Vec<_>>();
    if characters.len() < 3 {
        return false;
    }
    let separators = "-_=~*·•—–. ";
    characters
        .iter()
        .filter(|character| separators.contains(**character))
        .count()
        * 2
        >= characters.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Page, PageStatus};
    use crate::enums::OcrMode;

    fn page(index: usize, footer: &str, body: &str) -> Page {
        let blocks = vec![
            Block::new(1, 1, "Book title", 100, 40, 300, 20),
            Block::new(2, 1, body, 100, 400, 500, 30),
            Block::new(3, 1, footer, 100, 900, 300, 20),
        ];
        Page {
            index,
            path: format!("{index}.png"),
            status: PageStatus::OcrSucceeded,
            raw_text: blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            raw_output: String::new(),
            raw_output_mode: OcrMode::Text,
            filtered_blocks: blocks.clone(),
            blocks,
            page_width: 1000,
            page_height: 1000,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn removes_repeated_furniture_but_keeps_ambiguous_numeric_content() {
        let mut document = Document {
            pages: vec![
                page(0, "Page 1", "1. A numeric footnote remains"),
                page(1, "Page 2", "1. A numeric footnote remains"),
                page(2, "Page 3", "1. A numeric footnote remains"),
            ],
        };

        let exclusions = filter_document(&mut document);

        assert_eq!(exclusions.len(), 6);
        for page in &document.pages {
            assert_eq!(
                page.filtered_blocks[0].text,
                "1. A numeric footnote remains"
            );
            assert_eq!(page.filtered_blocks.len(), 1);
        }
        assert!(exclusions.iter().any(|item| {
            item.reason == ExclusionReason::RepeatedTopLine
                && item.text == "Book title"
                && item.page == 1
        }));
        assert!(exclusions.iter().any(|item| {
            item.reason == ExclusionReason::FixedPositionPageNumber && item.text == "Page 2"
        }));
    }

    #[test]
    fn removes_separator_footer_only_at_the_bottom() {
        let mut document = Document {
            pages: vec![page(0, "---", "body"), page(1, "---", "body")],
        };

        let exclusions = filter_document(&mut document);

        assert!(
            exclusions
                .iter()
                .any(|item| { item.reason == ExclusionReason::SeparatorLikeFooter })
        );
        assert!(
            document
                .pages
                .iter()
                .all(|page| { page.filtered_blocks.iter().all(|block| block.text != "---") })
        );
    }

    #[test]
    fn preserves_repeated_unmarked_numeric_footer() {
        let mut document = Document {
            pages: vec![
                page(0, "1.", "body one"),
                page(1, "1.", "body two"),
                page(2, "1.", "body three"),
            ],
        };

        let exclusions = filter_document(&mut document);

        assert!(exclusions.iter().all(|item| item.text != "1."));
        assert!(
            document
                .pages
                .iter()
                .all(|page| { page.filtered_blocks.iter().any(|block| block.text == "1.") })
        );
    }

    #[test]
    fn removes_unmarked_numbers_only_with_sequential_page_evidence() {
        let mut document = Document {
            pages: vec![
                page(0, "10.", "body one"),
                page(1, "11.", "body two"),
                page(2, "12.", "body three"),
            ],
        };

        let exclusions = filter_document(&mut document);

        assert_eq!(
            exclusions
                .iter()
                .filter(|item| item.reason == ExclusionReason::FixedPositionPageNumber)
                .count(),
            3
        );
    }

    #[test]
    fn removes_ocr_variants_of_repeated_headers() {
        let mut first = page(0, "footer one", "body one");
        first.blocks[0].text = "RUSCOVA - o comună ucraineană din Maramureș".to_string();
        first.page_width = 843;
        first.blocks[0].coordinates = Coordinates {
            left: 119,
            top: 72,
            width: 697,
            height: 28,
        };
        let mut second = page(1, "footer two", "body two");
        second.blocks[0].text =
            "RUSCOVA —o comună ucraineană din Maramureş ——————————————".to_string();
        second.page_width = 804;
        second.blocks[0].coordinates = Coordinates {
            left: 124,
            top: 127,
            width: 663,
            height: 39,
        };
        let mut document = Document {
            pages: vec![first, second],
        };

        let exclusions = filter_document(&mut document);

        assert_eq!(
            exclusions
                .iter()
                .filter(|item| item.reason == ExclusionReason::RepeatedTopLine)
                .count(),
            2
        );
        assert!(document.pages.iter().all(|page| {
            page.filtered_blocks
                .iter()
                .all(|block| !block.text.starts_with("RUSCOVA"))
        }));
    }

    #[test]
    fn preserves_a_one_word_top_line_against_a_long_header_match() {
        let mut header_page = page(0, "footer one", "body one");
        header_page.blocks[0].text = "RUSCOVA o comună ucraineană din Maramureș".to_string();
        let mut body_page = page(1, "footer two", "body two");
        body_page.blocks[0].text = "RUSCOVA".to_string();
        let mut document = Document {
            pages: vec![header_page, body_page],
        };

        let exclusions = filter_document(&mut document);

        assert!(exclusions.iter().all(|item| !item.text.contains("RUSCOVA")));
        assert!(
            document.pages[0]
                .filtered_blocks
                .iter()
                .any(|block| block.text.starts_with("RUSCOVA o comună"))
        );
        assert!(
            document.pages[1]
                .filtered_blocks
                .iter()
                .any(|block| block.text == "RUSCOVA")
        );
    }

    #[test]
    fn preserves_distinct_language_text_in_furniture_comparison() {
        let mut first = page(0, "footer one", "body one");
        first.blocks[0].text = "Știință din țară".to_string();
        let mut second = page(1, "footer two", "body two");
        second.blocks[0].text = "Stiinta din tara".to_string();
        let mut document = Document {
            pages: vec![first, second],
        };

        let exclusions = filter_document(&mut document);

        assert!(exclusions.is_empty());
        assert!(document.pages.iter().all(|page| {
            page.filtered_blocks
                .iter()
                .any(|block| block.text.contains("tiin"))
        }));
    }

    #[test]
    fn removes_sequential_page_numbers_with_different_punctuation() {
        let mut pages = vec![
            page(0, "-10-", "body one"),
            page(1, "-U-", "body two"),
            page(2, "12.", "body three"),
            page(3, "13", "body four"),
            page(4, "14.", "body five"),
        ];
        for (index, width, left, top, block_width) in [
            (0, 843, 405, 1191, 54),
            (2, 804, 428, 1193, 36),
            (3, 854, 380, 1181, 52),
            (4, 800, 417, 1189, 50),
        ] {
            let page = &mut pages[index];
            page.page_width = width;
            page.page_height = 1280;
            page.blocks[2].coordinates = Coordinates {
                left,
                top,
                width: block_width,
                height: 19,
            };
        }
        let mut document = Document { pages };

        let exclusions = filter_document(&mut document);

        assert_eq!(
            exclusions
                .iter()
                .filter(|item| item.reason == ExclusionReason::FixedPositionPageNumber)
                .count(),
            4
        );
        assert!(
            document.pages[1]
                .filtered_blocks
                .iter()
                .all(|block| block.text != "-U-")
        );
        assert!(exclusions.iter().any(|item| {
            item.text == "-U-" && item.reason == ExclusionReason::SeparatorLikeFooter
        }));
    }
}
