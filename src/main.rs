use afterprint::{
    RunEvent, RunStatus, SOURCE_PDF_FILENAME, TRANSLATED_PDF_FILENAME, cli::Args, run_cli,
};
use anyhow::{Result, bail};
use std::path::Path;

fn main() -> Result<()> {
    let args = Args::parse();
    let config = args.run_config()?;
    let report = run_cli(&config, |event| print_human_event(&config.output, &event))?;

    if report.status == RunStatus::PartialFailure {
        bail!("{} OCR page(s) failed", report.pages_failed);
    }
    Ok(())
}

fn print_human_event(output: &Path, event: &RunEvent) {
    match event {
        RunEvent::ArtifactWritten {
            path,
            page: Some(page),
        } if path.starts_with("translations/") => {
            println!(
                "wrote translations page {page} to {}",
                output.join(path).display()
            );
        }
        RunEvent::ArtifactWritten {
            path,
            page: Some(page),
        } if path.starts_with("filtered/") => println!("wrote page {page}"),
        RunEvent::ArtifactWritten { path, page: None }
            if path == SOURCE_PDF_FILENAME || path == TRANSLATED_PDF_FILENAME =>
        {
            println!("wrote {path}");
        }
        RunEvent::Warning { message, page } => match page {
            Some(page) => eprintln!("warning on page {page}: {message}"),
            None => eprintln!("warning: {message}"),
        },
        _ => {}
    }
}
