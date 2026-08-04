use anyhow::{Result, ensure};

// + '_ -> The iterator borrows from the argument source, so it must not outlive it
fn paragraphs(source: &str) -> impl Iterator<Item = String> + '_ {
    source.trim().split("\n\n").filter_map(|paragraph| {
        let paragraph = paragraph
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        // Some(paragraph) keeps it while None removes it from the iterator
        (!paragraph.is_empty()).then_some(paragraph)
    })
}

fn chunks(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    text.split_whitespace()
        .fold(Vec::new(), |mut chunks, word| {
            match chunks.last_mut() {
                Some(chunk) if chunk.len() + word.len() < max_chars => {
                    chunk.push(' ');
                    chunk.push_str(word);
                }
                _ => chunks.push(word.to_string()),
            }

            chunks
        })
}

pub fn translate_paragraphs<F>(source: &str, max_chars: usize, mut translate: F) -> Result<String>
where
    // F must be a callable function or closure that receives a borrowed slice of strings and
    // returns either a vector of strings or an error
    F: FnMut(&[String]) -> Result<Vec<String>>,
{
    // Remembers original chunk grouping to reassemble the translated text into paragraphs
    let units_by_paragraph = paragraphs(source)
        .map(|paragraph| chunks(&paragraph, max_chars))
        .collect::<Vec<_>>();
    // Flattened chunks: send this to the translate function
    let units = units_by_paragraph
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();

    if units.is_empty() {
        return Ok(String::new());
    }

    let translations = translate(&units)?;
    ensure!(
        translations.len() == units.len(),
        "translator returned {} results for {} units",
        translations.len(),
        units.len(),
    );
    let mut translations = translations.into_iter();
    let paragraphs = units_by_paragraph
        .iter()
        .map(|units| {
            units
                .iter()
                .map(|_| {
                    translations
                        .next()
                        .expect("translation count was validated")
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>();

    Ok(paragraphs.join("\n\n"))
}
