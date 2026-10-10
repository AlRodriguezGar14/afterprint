use afterprint::{
    OcrMode, PdfContent, RunConfig, RunEvent, RunStatus, SOURCE_PDF_FILENAME,
    TRANSLATED_PDF_FILENAME, run,
};
use serde_json::Value;
use std::{path::PathBuf, process::Command};

#[test]
fn library_api_runs_the_fixture_and_returns_typed_page_and_artifact_events() {
    let root = temp_dir("library-api");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli-scans");
    let config = RunConfig {
        input: fixture,
        output: root.join("output"),
        ocr_language: "eng".into(),
        ocr_mode: OcrMode::Text,
        title: "Synthetic fixture".into(),
        author: "afterprint test".into(),
        language: "en".into(),
        pdf: false,
        pdf_content: PdfContent::Filtered,
        translation: None,
    };
    let mut events = Vec::new();

    let report = run(&config, |event| events.push(event)).unwrap();

    assert_eq!(report.status, RunStatus::PartialFailure);
    assert_eq!(
        (
            report.pages_total,
            report.pages_succeeded,
            report.pages_failed
        ),
        (3, 2, 1)
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RunEvent::PageProgress { page: 2, total: 3 }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RunEvent::PageFailed { page: 2, .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        RunEvent::ArtifactWritten { path, page: Some(3) } if path == "0003.txt"
    )));
    assert!(matches!(
        events.last(),
        Some(RunEvent::RunFinished {
            status: RunStatus::PartialFailure,
            pages_total: 3,
            pages_succeeded: 2,
            pages_failed: 1,
        })
    ));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn jsonl_cli_emits_only_versioned_events_and_reports_the_failed_middle_page() {
    let root = temp_dir("jsonl");
    let output = root.join("output");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli-scans");

    let result = Command::new(env!("CARGO_BIN_EXE_afterprint"))
        .arg(fixture)
        .arg("--out")
        .arg(&output)
        .args(["--pdf", "--events", "jsonl"])
        .output()
        .unwrap();

    assert!(!result.status.success());
    let stdout = String::from_utf8(result.stdout).unwrap();
    let stderr = String::from_utf8(result.stderr).unwrap();
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(!events.is_empty());
    assert!(events.iter().all(|event| event["schema_version"] == 1));
    for event in &events {
        match event["event"].as_str().unwrap() {
            "stage" => assert!(event["stage"].is_string()),
            "page_progress" => {
                assert!(event["page"].is_u64());
                assert!(event["total"].is_u64());
            }
            "warning" => assert!(event["message"].is_string()),
            "page_failed" => {
                assert!(event["page"].is_u64());
                assert!(event["message"].is_string());
            }
            "setup_progress" => {
                assert!(event["component"].is_string());
                assert!(event["stage"].is_string());
                assert!(event["message"].is_string());
            }
            "artifact_written" => assert!(event["path"].is_string()),
            "run_finished" => {
                assert!(event["status"].is_string());
                assert!(event["pages_total"].is_u64());
                assert!(event["pages_succeeded"].is_u64());
                assert!(event["pages_failed"].is_u64());
            }
            event => panic!("unexpected JSONL event: {event}"),
        }
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "page_progress")
            .map(|event| event["page"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(events.iter().any(|event| {
        event["event"] == "artifact_written" && event["path"] == "0001.txt" && event["page"] == 1
    }));
    assert!(events.iter().any(|event| {
        event["event"] == "page_failed" && event["page"] == 2 && event["message"].is_string()
    }));
    assert!(events.iter().any(|event| {
        event["event"] == "artifact_written" && event["path"] == SOURCE_PDF_FILENAME
    }));
    assert_eq!(
        events.last().unwrap(),
        &serde_json::json!({
            "schema_version": 1,
            "event": "run_finished",
            "status": "partial_failure",
            "pages_total": 3,
            "pages_succeeded": 2,
            "pages_failed": 1,
        })
    );
    assert!(stderr.contains("1 OCR page(s) failed"));
    assert!(output.join("debug/processing.json").is_file());
    assert!(output.join(SOURCE_PDF_FILENAME).is_file());
    assert!(output.join("0002.txt").is_file());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn human_cli_keeps_page_messages_and_partial_failure_exit() {
    let root = temp_dir("human-cli");
    let output = root.join("output");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli-scans");

    let result = Command::new(env!("CARGO_BIN_EXE_afterprint"))
        .arg(fixture)
        .arg("--out")
        .arg(&output)
        .output()
        .unwrap();

    assert!(!result.status.success());
    let stdout = String::from_utf8(result.stdout).unwrap();
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(stdout.contains("wrote page 1"));
    assert!(stdout.contains("wrote page 2"));
    assert!(stderr.contains("1 OCR page(s) failed"));
    assert!(output.join("debug/processing.json").is_file());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn generated_pdf_filenames_are_shared_public_contract() {
    assert_eq!(SOURCE_PDF_FILENAME, "source.pdf");
    assert_eq!(TRANSLATED_PDF_FILENAME, "translated.pdf");
}

#[test]
fn jsonl_does_not_prompt_for_a_missing_translation_model() {
    let root = temp_dir("jsonl-model-setup");
    let output = root.join("output");
    let home = root.join("home");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli-scans");

    let result = Command::new(env!("CARGO_BIN_EXE_afterprint"))
        .arg(fixture)
        .arg("--out")
        .arg(&output)
        .args([
            "--translation-model",
            "opus",
            "--translation-source",
            "ro",
            "--translation-target",
            "en",
            "--events",
            "jsonl",
        ])
        .env("HOME", home)
        .output()
        .unwrap();

    assert!(!result.status.success());
    let stdout = String::from_utf8(result.stdout).unwrap();
    let stderr = String::from_utf8(result.stderr).unwrap();
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], "run_finished");
    assert_eq!(events[0]["status"], "failure");
    assert!(stderr.contains("missing config.json"));
    assert!(!stdout.contains("Try to download"));

    std::fs::remove_dir_all(root).unwrap();
}

fn temp_dir(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "afterprint-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}
