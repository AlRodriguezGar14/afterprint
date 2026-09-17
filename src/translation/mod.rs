use anyhow::{Context, Result};
use camino::Utf8PathBuf;

mod ct2;
mod opus_pairs;
mod text;

pub use ct2::TranslationArgs;
pub(crate) use text::translate_segments;

/// Writes the page-addressable projection of a logical translation.
pub fn write_translations(
    out: &Utf8PathBuf,
    index: usize,
    source: &str,
    translation: &str,
) -> Result<Utf8PathBuf> {
    let output_dir = out.join("translations");
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create translations directory {output_dir}"))?;

    let output_path = output_dir.join(format!("{:04}.txt", index + 1));
    let output = format!("Original:\n{source}\n\n---\n\nTranslation:\n{translation}\n");

    std::fs::write(&output_path, output)
        .with_context(|| format!("failed to write translations to {output_path}"))?;

    Ok(output_path)
}
