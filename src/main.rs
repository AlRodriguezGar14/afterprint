use anyhow::{Result, bail};

use crate::cli::Args;
use crate::document::Document;
use crate::filter::filter_document;
use crate::ocr::ocr_pages_in_dir;
use crate::output::write_document_outputs;
use crate::translation::write_translations;

mod cli;
mod document;
mod enums;
mod filter;
mod ocr;
mod output;
mod translation;

fn main() -> Result<()> {
    let args = Args::parse();

    let translator = args.translation.open()?;

    let mut document = Document {
        pages: ocr_pages_in_dir(&args.image_folder, &args.ocr_language, args.ocr_mode).collect(),
    };
    let exclusions = filter_document(&mut document);
    let failure_count = write_document_outputs(&args.out, &document, &exclusions)?;

    for page in &document.pages {
        let source = page.raw_text.trim_end();

        if !source.trim().is_empty()
            && let Some(translator) = &translator
        {
            let translation = translator.translate_page(source)?;

            match write_translations(&args.out, page.index, source, &translation) {
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
    }

    if failure_count != 0 {
        bail!("{failure_count} OCR page(s) failed");
    }
    Ok(())
}
