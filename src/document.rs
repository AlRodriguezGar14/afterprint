use serde::Serialize;

#[derive(Debug)]
pub struct Document {
    pub pages: Vec<Page>,
}

#[derive(Clone, Debug)]
pub struct Page {
    pub index: usize,
    pub path: String,
    pub status: PageStatus,
    /// Raw OCR stays untouched; filtered output is derived from `filtered_blocks`.
    pub raw_text: String,
    pub raw_output: String,
    pub raw_output_mode: crate::enums::OcrMode,
    pub blocks: Vec<Block>,
    pub filtered_blocks: Vec<Block>,
    pub page_width: u32,
    pub page_height: u32,
    pub warnings: Vec<String>,
}

impl Page {
    /// Derives plain text from the page's retained OCR blocks. When nothing was
    /// excluded, `raw_text` is preserved; this method writes nothing.
    pub fn filtered_text(&self) -> String {
        if self.filtered_blocks.len() == self.blocks.len() {
            return self.raw_text.clone();
        }

        let mut text = self
            .filtered_blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if self.raw_text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Block {
    pub block_num: u32,
    pub line_num: u32,
    pub text: String,
    pub coordinates: Coordinates,
    pub confidence: f32,
}

impl Block {
    #[cfg(test)]
    pub fn new(
        block_num: u32,
        line_num: u32,
        text: &str,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    ) -> Self {
        Self {
            block_num,
            line_num,
            text: text.to_string(),
            coordinates: Coordinates {
                left,
                top,
                width,
                height,
            },
            confidence: 90.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Coordinates {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum PageStatus {
    OcrSucceeded,
    OcrFailed { error: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::OcrMode;

    fn page(blocks: Vec<Block>, raw_text: &str) -> Page {
        page_at(0, blocks, raw_text)
    }

    fn page_at(index: usize, blocks: Vec<Block>, raw_text: &str) -> Page {
        Page {
            index,
            path: "page.png".to_string(),
            status: PageStatus::OcrSucceeded,
            raw_text: raw_text.to_string(),
            raw_output: raw_text.to_string(),
            raw_output_mode: OcrMode::Text,
            filtered_blocks: blocks.clone(),
            blocks,
            page_width: 100,
            page_height: 100,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn filtered_text_keeps_raw_layout_when_no_block_was_removed() {
        let page = page(vec![Block::new(1, 1, "body", 0, 20, 40, 10)], "body\n");

        assert_eq!(page.filtered_text(), "body\n");
    }

    #[test]
    fn failed_pages_can_remain_in_the_document() {
        let page = Page {
            index: 1,
            path: "missing.png".to_string(),
            status: PageStatus::OcrFailed {
                error: "OCR recognition failed".to_string(),
            },
            raw_text: String::new(),
            raw_output: String::new(),
            raw_output_mode: OcrMode::Text,
            blocks: Vec::new(),
            filtered_blocks: Vec::new(),
            page_width: 0,
            page_height: 0,
            warnings: Vec::new(),
        };
        let document = Document { pages: vec![page] };

        assert_eq!(document.pages[0].index, 1);
        assert!(matches!(
            document.pages[0].status,
            PageStatus::OcrFailed { .. }
        ));
    }

    #[test]
    fn page_identity_and_raw_text_survive_a_middle_failure() {
        let middle = Page {
            index: 1,
            path: "middle.png".to_string(),
            status: PageStatus::OcrFailed {
                error: "OCR recognition failed".to_string(),
            },
            raw_text: String::new(),
            raw_output: String::new(),
            raw_output_mode: OcrMode::Text,
            blocks: Vec::new(),
            filtered_blocks: Vec::new(),
            page_width: 0,
            page_height: 0,
            warnings: Vec::new(),
        };
        let document = Document {
            pages: vec![
                page_at(0, Vec::new(), "first"),
                middle,
                page_at(2, Vec::new(), "third"),
            ],
        };

        assert_eq!(
            document
                .pages
                .iter()
                .map(|page| page.index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(document.pages[0].raw_text, "first");
        assert_eq!(document.pages[2].raw_text, "third");
        assert!(matches!(
            document.pages[1].status,
            PageStatus::OcrFailed { .. }
        ));
    }
}
