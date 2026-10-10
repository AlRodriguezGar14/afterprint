use afterprint::{
    RunEvent, RunStatus, SOURCE_PDF_FILENAME, TRANSLATED_PDF_FILENAME,
    cli::{Args, EventFormat},
    run, run_cli,
};
use anyhow::{Result, bail};
use std::io::{self, Write};

fn main() -> Result<()> {
    let args = Args::parse();
    let config = args.run_config()?;
    let jsonl = args.events == Some(EventFormat::Jsonl);
    let mut output_error = None;
    let report = if jsonl {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        let mut emit = |event: RunEvent| {
            if let Err(error) = write_jsonl_event(&event, &mut stdout) {
                output_error.get_or_insert(error);
            }
        };
        run(&config, &mut emit)?
    } else {
        run_cli(&config, |event| print_human_event(&config.output, &event))?
    };

    if let Some(error) = output_error {
        return Err(error.into());
    }
    if report.status == RunStatus::PartialFailure {
        bail!("{} OCR page(s) failed", report.pages_failed);
    }
    Ok(())
}

fn write_jsonl_event(event: &RunEvent, stdout: &mut impl Write) -> io::Result<()> {
    let line = event.to_jsonl().map_err(io::Error::other)?;
    writeln!(stdout, "{line}")
}

fn print_human_event(output: &std::path::Path, event: &RunEvent) {
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
