# afterprint

Some books disappear not because they were forgotten, but because no usable
digital copy survives.

**afterprint** is a slow-built labor of love and a small contribution to
preserving lost media. It turns scans of rare and out-of-print books into
paragraph-aware text and optional local translations, so they can be read,
searched, and shared again.

Paragraphs stay intact through recognition and translation. The application is
open source, and the pipeline runs locally.

## Setup — once

Install and start Docker with Compose, download this repository, and open a
terminal in its folder. Build the app:

```bash
docker compose build afterprint
```

This installs everything inside Docker and takes several minutes the first
time. You don't need Rust or Python on your computer. Once it finishes, you're
ready to process books; no background service needs starting.

The commands below work on macOS, Linux, and Windows through WSL.

## Use — for each book

Put your page images in a folder, one PNG or JPEG per page, named in reading
order (`001.png`, `002.png`, …). Convert PDFs to images first.

This command reads a Romanian book and translates it into English:

```bash
./afterprint "/path/to/book scans" --out ./results/my-book \
  --ocr-language ron \
  --translation-model opus --translation-source ro --translation-target en
```

Add `--pdf` to write aligned PDFs alongside the page text. The default is
filtered OCR; use `--pdf-content raw` to render untouched OCR instead:

```bash
./afterprint "/path/to/book scans" --out ./results/my-book --pdf
```

The output includes `source.pdf`, and `translated.pdf` when translation is
enabled. Both PDFs contain one fixed A4 page per input image; OCR failures
remain blank pages so page numbers stay aligned.

Replace the scans path and output folder with your own. Keep quotes around
paths containing spaces. The output folder is created automatically; keep it
outside your scans folder.

**Set the languages:** `--ocr-language` tells the app which language is printed
on the pages. `--translation-source` and `--translation-target` set the
translation **from** and **to**. They use different codes:

| Language | OCR code | Translation code |
| -------- | -------- | ---------------- |
| English  | `eng`    | `en`             |
| Spanish  | `spa`    | `es`             |
| Japanese | `jpn`    | `ja`             |
| Romanian | `ron`    | `ro`             |

For Spanish → English, change `ron` to `spa` and `ro` to `es`.
For OCR without translation, omit the three `--translation-*` options.
English OCR and `./out` are the defaults if you omit their options.

**Choose a model:** `opus` uses smaller models for supported language pairs.
If your pair is unsupported, use `nllb` for the app's 93-language catalog.
NLLB is experimental and noncommercial; see [License](#license).

### First translation vs. everyday runs

The first time you use a model, afterprint asks to download and convert it.
Type `y` and press Enter at the prompts. This needs internet; the model is
then saved in the project's `.afterprint/` folder.

For the next book, run the same command with new input and output paths.
**There is no rebuild or repeat download:** the launcher reuses the app and
cached model. You wait only for model loading and page processing, which
depends on the book's size and your computer.

Keep Docker running, the built image, and `.afterprint/`. They survive a
restart; cached models also work offline. Rebuild only after updating the app
or changing its installed OCR languages, by adding `--build` to your command.
If the image is missing, the launcher builds it automatically.

### Results

Open the output folder when the command finishes:

- `0001.txt`, `0002.txt`, … contain the recognized page text.
- `filtered/0001.txt`, `filtered/0002.txt`, … contain filtered page text.
- `translations/0001.txt`, … contain the original and translated text for
  pages with recognized text.
- `source.pdf` contains filtered text by default; `translated.pdf` is written
  for translated runs.

These are editable text files, not a combined PDF or ebook. Check them against
your scans: recognition can make mistakes or skip unreadable pages.

Use a separate output folder for each book. Rerunning starts from page one,
overwrites matching files, and leaves unmatched old files in place. Save any
manual edits elsewhere before rerunning. Press `Ctrl+C` to stop processing.

## Other OCR languages

English, Spanish, Japanese, and Romanian OCR are included. To build with a
different set, supply Tesseract language codes:

```bash
OCR_LANGUAGES="eng deu fra" ./afterprint ./scans --out ./results --ocr-language deu --build
```

Normal runs retain that set without rebuilding. Supply it again on future
rebuilds to keep it; otherwise the four defaults return. This setting affects
OCR only, not translation language choices.

## Help

Run `./afterprint --help` for launcher usage or `./afterprint --app-help` for
all options, including custom models. If you get `Permission denied` when
starting the launcher, use `bash ./afterprint` instead.

<details>
<summary>Native development setup (macOS, without Docker)</summary>

```bash
brew install rust cmake tesseract tesseract-lang leptonica pkg-config
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -U pip ctranslate2 huggingface_hub torch sentencepiece sacremoses "transformers<5"
cargo run --release -- ./scans --out ./out --ocr-language eng
```

Native runs cache models in `~/.config/afterprint/models`.

</details>

## License

Source code: [MIT](LICENSE). Translation models retain their own licenses:

- [NLLB-200 distilled 600M](https://huggingface.co/facebook/nllb-200-distilled-600M):
  CC BY-NC 4.0 (noncommercial). Meta describes it as a research model not intended
  for production or document translation; this integration is experimental.
- OPUS: terms vary by checkpoint; review the selected Helsinki-NLP model card.
