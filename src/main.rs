use anyhow::Result;

use crate::cli::Args;
use crate::ocr::{collect_recognized_text, ocr_pages_in_dir, write_page};

mod cli;
mod enums;
mod ocr;

fn main() -> Result<()> {
    let args = Args::parse();

    std::fs::create_dir_all(&args.out).unwrap();

    for (index, text) in ocr_pages_in_dir(&args.image_folder, &args.ocr_language)
        .filter_map(|mut api| collect_recognized_text(&mut api, args.ocr_mode))
        .enumerate()
    {
        match write_page(&args.out, index, args.ocr_mode, &text) {
            Ok(output_path) => println!("wrote page {} to {output_path}", index + 1),
            Err(error) => eprintln!("failed to write page {}. Err: {error}", index + 1),
        }
    }

    Ok(())
}
