use anyhow::{Result, bail};

use crate::cli::Args;
use crate::document::Document;
use crate::filter::filter_document;
use crate::ocr::ocr_pages_in_dir;
use crate::output::write_document_outputs;
use crate::pdf::write_pdf;
use crate::segments::{build_segments, map_translations_to_pages};
use crate::translation::write_translations;

mod cli;
mod document;
mod enums;
mod filter;
mod ocr;
mod output;
mod pdf;
mod segments;
mod translation;

fn main() -> Result<()> {
    let args = Args::parse();

    let translator = args.translation.open()?;

    let mut document = Document {
        pages: ocr_pages_in_dir(&args.image_folder, &args.ocr_language, args.ocr_mode).collect(),
    };
    let exclusions = filter_document(&mut document);
    let segments = build_segments(&document);
    let failure_count = write_document_outputs(&args.out, &document, &exclusions, &segments)?;

    if args.pdf {
        let page_texts = document
            .pages
            .iter()
            .map(|page| args.pdf_content.text(page))
            .collect::<Vec<_>>();
        write_pdf(
            &args.out,
            "source.pdf",
            &document,
            &page_texts,
            &args.title,
            &args.author,
            &args.language,
        )?;
    }

    if let Some(translator) = &translator {
        let sources = segments
            .iter()
            .map(|segment| segment.source_text.clone())
            .collect::<Vec<_>>();
        let translations = translator.translate_segments(&sources)?;
        let page_translations =
            map_translations_to_pages(&segments, &translations, document.pages.len())?;

        for page in &document.pages {
            let source = page.filtered_text();
            let translation = page_translations
                .get(page.index)
                .map(String::as_str)
                .unwrap_or_default();
            if source.trim().is_empty() || translation.trim().is_empty() {
                continue;
            }

            match write_translations(&args.out, page.index, source.trim_end(), translation) {
                Ok(output_path) => {
                    println!(
                        "wrote translations page {} to {output_path}",
                        page.index + 1
                    )
                }
                Err(error) => eprintln!(
                    "failed to write translations page {}. Err: {error}",
                    page.index + 1
                ),
            }
        }

        if args.pdf {
            write_pdf(
                &args.out,
                "translated.pdf",
                &document,
                &page_translations,
                &args.title,
                &args.author,
                &args.language,
            )?;
        }
    }

    if failure_count != 0 {
        bail!("{failure_count} OCR page(s) failed");
    }
    Ok(())
}
