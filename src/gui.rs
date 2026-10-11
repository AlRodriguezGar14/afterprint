use crate::{
    OcrMode, PdfContent, RunConfig, RunEvent, RunReport,
    cli::{Args, TESSERACT_LANGUAGES},
    run as run_pipeline,
};
use iced::{
    Alignment, Color, Element, Length, Task, Theme,
    theme::Palette,
    widget::{button, column, container, row, rule, text, text_input},
};
use std::path::{Path, PathBuf};
use tesseract::Tesseract;

const MAX_LANGUAGE_SUGGESTIONS: usize = 8;

/// Runs the native GUI, prefilled with the scan, output, and OCR language options.
pub fn run(args: Args) -> iced::Result {
    let initial = App {
        input: args.image_folder.map(|path| path.into_std_path_buf()),
        output: args.out.map(|path| path.into_std_path_buf()),
        ocr_language: args.ocr_language,
        show_language_suggestions: false,
        status: String::new(),
        running: false,
    };

    iced::application(move || initial.clone(), update, view)
        .title("afterprint")
        .theme(|_: &App| paper_theme())
        .window_size((820.0, 760.0))
        .run()
}

#[derive(Clone)]
struct App {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    ocr_language: String,
    show_language_suggestions: bool,
    status: String,
    running: bool,
}

#[derive(Debug, Clone)]
enum Message {
    ChooseInput,
    InputSelected(Option<PathBuf>),
    ChooseOutput,
    OutputSelected(Option<PathBuf>),
    LanguageSearchChanged(String),
    LanguageSelected(String),
    Run,
    RunFinished(String),
}

fn update(state: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::ChooseInput => Task::perform(
            rfd::AsyncFileDialog::new()
                .set_title("Choose scan folder")
                .pick_folder(),
            |folder| Message::InputSelected(folder.map(|folder| folder.path().to_path_buf())),
        ),
        Message::InputSelected(path) => {
            state.input = path;
            Task::none()
        }
        Message::ChooseOutput => Task::perform(
            rfd::AsyncFileDialog::new()
                .set_title("Choose output folder")
                .pick_folder(),
            |folder| Message::OutputSelected(folder.map(|folder| folder.path().to_path_buf())),
        ),
        Message::OutputSelected(path) => {
            state.output = path;
            Task::none()
        }
        Message::LanguageSearchChanged(language) => {
            state.ocr_language = language.to_ascii_lowercase();
            state.show_language_suggestions = true;
            state.status.clear();
            Task::none()
        }
        Message::LanguageSelected(language) => {
            state.ocr_language = language;
            state.show_language_suggestions = false;
            state.status.clear();
            Task::none()
        }
        Message::Run => {
            let Some(input) = state.input.clone() else {
                state.status = "Choose a scan folder first.".into();
                return Task::none();
            };
            let Some(output) = state.output.clone() else {
                state.status = "Choose an output folder first.".into();
                return Task::none();
            };
            if !is_supported_language(&state.ocr_language) {
                state.status =
                    "Choose a supported Tesseract language code from the matches.".into();
                return Task::none();
            }
            match output_is_empty(&output) {
                Ok(true) => {}
                Ok(false) => {
                    state.status = "Choose an empty output folder to avoid replacing files.".into();
                    return Task::none();
                }
                Err(error) => {
                    state.status = format!("Cannot inspect output folder: {error}");
                    return Task::none();
                }
            }

            let language = state.ocr_language.clone();
            state.show_language_suggestions = false;
            state.running = true;
            state.status = "Running OCR…".into();
            Task::perform(
                async move {
                    std::thread::spawn(move || {
                        preflight_language(&language)?;
                        let config = RunConfig {
                            input,
                            output,
                            ocr_language: language,
                            ocr_mode: OcrMode::Text,
                            title: "Untitled".into(),
                            author: "Unknown".into(),
                            language: "en".into(),
                            pdf: false,
                            pdf_content: PdfContent::Filtered,
                            translation: None,
                        };
                        let mut page_errors = Vec::new();
                        run_pipeline(&config, |event| {
                            if let RunEvent::PageFailed { page, message } = event {
                                page_errors.push((page, message));
                            }
                        })
                        .map(|report| (report, page_errors))
                        .map_err(|error| error.to_string())
                    })
                    .join()
                    .unwrap_or_else(|_| Err("Processing worker failed.".into()))
                },
                |result| Message::RunFinished(format_report(result)),
            )
        }
        Message::RunFinished(status) => {
            state.running = false;
            state.status = status;
            Task::none()
        }
    }
}

fn view(state: &App) -> Element<'_, Message> {
    let language_suggestions: Vec<Element<'_, Message>> = if state.show_language_suggestions {
        let matches = matching_language_codes(&state.ocr_language);
        if matches.is_empty() {
            vec![
                text("No supported codes match this search.")
                    .size(14)
                    .into(),
            ]
        } else {
            matches
                .into_iter()
                .map(|language| {
                    button(language)
                        .on_press(Message::LanguageSelected(language.to_owned()))
                        .into()
                })
                .collect()
        }
    } else {
        Vec::new()
    };
    let language_status = if is_supported_language(&state.ocr_language) {
        format!("Selected: {}", state.ocr_language)
    } else {
        "Choose a supported code from the matches.".to_owned()
    };

    container(
        column![
            text("afterprint").size(28),
            column![
                text("Make page scans readable.").size(28),
                text("Choose folders and a Tesseract language installed on this Mac."),
            ]
            .spacing(8),
            rule::horizontal(2),
            column![
                text("01  SCAN FOLDER").size(12),
                row![
                    text(state.input.as_ref().map_or_else(
                        || "Choose a folder of page images".to_owned(),
                        |path| path.display().to_string(),
                    )),
                    button("Choose folder").on_press(Message::ChooseInput),
                ]
                .spacing(16)
                .align_y(Alignment::Center),
            ]
            .spacing(8),
            rule::horizontal(1),
            column![
                text("02  OUTPUT FOLDER").size(12),
                row![
                    text(state.output.as_ref().map_or_else(
                        || "Choose a new or empty folder".to_owned(),
                        |path| path.display().to_string(),
                    )),
                    button("Choose folder").on_press(Message::ChooseOutput),
                ]
                .spacing(16)
                .align_y(Alignment::Center),
            ]
            .spacing(8),
            rule::horizontal(1),
            column![
                text("03  OCR LANGUAGE").size(12),
                text_input(
                    "Search language code (e.g. eng or ron)",
                    &state.ocr_language
                )
                .on_input(Message::LanguageSearchChanged),
                row(language_suggestions).spacing(8),
                text(language_status).size(14),
                text("Language data must be installed in Tesseract on this Mac.").size(14),
            ]
            .spacing(8),
            button(if state.running {
                "Running…"
            } else {
                "Run OCR"
            })
            .on_press_maybe((!state.running).then_some(Message::Run)),
            text(if state.status.is_empty() {
                "Ready."
            } else {
                state.status.as_str()
            })
            .size(14),
        ]
        .spacing(24)
        .width(Length::Fill)
        .max_width(720),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .padding(32)
    .into()
}

fn paper_theme() -> Theme {
    Theme::custom(
        "afterprint",
        Palette {
            background: Color::from_rgb8(247, 245, 238),
            text: Color::from_rgb8(31, 30, 27),
            primary: Color::from_rgb8(31, 30, 27),
            success: Color::from_rgb8(52, 83, 59),
            danger: Color::from_rgb8(143, 47, 40),
            warning: Color::from_rgb8(132, 91, 31),
        },
    )
}

fn preflight_language(language: &str) -> Result<(), String> {
    Tesseract::new(None, Some(language))
        .map(|_| ())
        .map_err(|_| missing_language_message(language))
}

fn matching_language_codes(query: &str) -> Vec<&'static str> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    TESSERACT_LANGUAGES
        .iter()
        .copied()
        .filter(|language| language.contains(&query))
        .take(MAX_LANGUAGE_SUGGESTIONS)
        .collect()
}

fn is_supported_language(language: &str) -> bool {
    TESSERACT_LANGUAGES.contains(&language)
}

fn missing_language_message(language: &str) -> String {
    format!(
        "Tesseract could not load `{language}`. Install `{language}.traineddata` or choose another language."
    )
}

fn output_is_empty(path: &Path) -> std::io::Result<bool> {
    match std::fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn format_report(result: Result<(RunReport, Vec<(usize, String)>), String>) -> String {
    match result {
        Ok((report, errors)) => {
            let summary = format!(
                "Finished: {:?}; {} page(s), {} succeeded, {} failed.",
                report.status, report.pages_total, report.pages_succeeded, report.pages_failed
            );
            errors.first().map_or(summary.clone(), |(page, error)| {
                format!("{summary} First page error (page {page}): {error}")
            })
        }
        Err(error) => format!("Run failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_LANGUAGE_SUGGESTIONS, format_report, is_supported_language, matching_language_codes,
        output_is_empty, preflight_language,
    };
    use crate::cli::TESSERACT_LANGUAGES;
    use crate::{RunReport, RunStatus};

    #[test]
    fn gui_requires_an_empty_output_folder() {
        let path = std::env::temp_dir().join(format!(
            "afterprint-gui-output-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        assert!(output_is_empty(&path).unwrap());
        std::fs::write(path.join("keep.txt"), "keep").unwrap();
        assert!(!output_is_empty(&path).unwrap());
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn gui_result_shows_the_first_failed_page() {
        let report = RunReport {
            status: RunStatus::PartialFailure,
            pages_total: 1,
            pages_succeeded: 0,
            pages_failed: 1,
            artifacts: Vec::new(),
        };
        let status = format_report(Ok((report, vec![(1, "language data missing".into())])));

        assert!(status.contains("page 1"));
        assert!(status.contains("language data missing"));
    }

    #[test]
    fn language_search_uses_supported_codes_and_excludes_rom() {
        assert!(TESSERACT_LANGUAGES.contains(&"ron"));
        assert!(!TESSERACT_LANGUAGES.contains(&"rom"));
        assert_eq!(matching_language_codes("RON"), vec!["ron"]);
        assert!(matching_language_codes("rom").is_empty());
        assert!(is_supported_language("ron"));
        assert!(!is_supported_language("rom"));
        assert_eq!(matching_language_codes("e").len(), MAX_LANGUAGE_SUGGESTIONS);
    }

    #[test]
    fn missing_language_data_fails_preflight_before_processing() {
        let language = "__afterprint_unavailable_test_language__";
        let error = preflight_language(language).unwrap_err();
        assert!(error.contains(&format!("Tesseract could not load `{language}`")));
        assert!(error.contains(&format!("{language}.traineddata")));
    }
}
