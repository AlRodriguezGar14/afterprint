// ocr
use leptess::{leptonica, tesseract};

// error handling
use anyhow::{Context, Result};

// cli
use camino::Utf8PathBuf;
use clap::Parser;

// navigate, iterate
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    pub image_folder: Utf8PathBuf,

    #[arg(short, long)]
    pub out: Utf8PathBuf,

    #[arg(short, long, default_value = "Untitled")]
    pub title: String,

    #[arg(short, long, default_value = "Unknown")]
    pub author: String,

    #[arg(short, long, default_value = "en")]
    pub language: String,

    #[arg(long, default_value = "eng")]
    pub ocr_language: String,

    #[arg(long, default_value = "a4")]
    pub page_size: String,

    #[arg(long)]
    pub force_ocr: bool,

    #[arg(long)]
    pub force_text: bool,

    #[arg(long)]
    pub no_pdf: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut api = tesseract::TessApi::new(None, &args.ocr_language)
        .context("failed to initialize Tesseract OCR")?;

    std::fs::create_dir_all(&args.out).unwrap();

    for (index, text) in WalkDir::new(&args.image_folder)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let path = entry.into_path();

            let Ok(pix) = leptonica::pix_read(&path) else {
                return None;
            };

            println!("\n>>>>{}<<<<<", path.display());

            api.set_image(&pix);

            let Ok(text) = api.get_utf8_text() else {
                return None;
            };

            Some(text)
        })
        .enumerate()
    {
        println!("{text}");

        let filename = format!("{:04}.txt", index + 1);
        let output_path = args.out.join(filename);

        if let Err(error) = std::fs::write(&output_path, text.as_bytes()) {
            eprintln!(
                "failed to write page {} to {output_path}: {error}",
                index + 1
            );
        }
    }

    Ok(())
}
