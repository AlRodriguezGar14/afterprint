use anyhow::Result;

use crate::cli::Args;
use crate::ocr::{collect_recognized_text, ocr_pages_in_dir, write_page};
use crate::translation::write_translations;

mod cli;
mod enums;
mod ocr;
mod translation;

fn main() -> Result<()> {
    let args = Args::parse();

    let translator = args.translation.open()?;
    std::fs::create_dir_all(&args.out)?;

    for (index, text) in ocr_pages_in_dir(&args.image_folder, &args.ocr_language)
        .filter_map(|mut api| collect_recognized_text(&mut api, args.ocr_mode))
        .enumerate()
    {
        let source = text.trim_end();

        if !source.trim().is_empty()
            && let Some(translator) = &translator
        {
            let translation = translator.translate_page(source)?;

            match write_translations(&args.out, index, source, &translation) {
                Ok(output_path) => {
                    println!("wrote translations page {} to {output_path}", index + 1)
                }
                Err(error) => eprintln!(
                    "failed to write translations page {}. Err: {error}",
                    index + 1
                ),
            }
        }

        match write_page(&args.out, index, args.ocr_mode, &text) {
            Ok(output_path) => println!("wrote page {} to {output_path}", index + 1),
            Err(error) => eprintln!("failed to write page {}. Err: {error}", index + 1),
        }
    }

    Ok(())
}
