# afterprint

Some books disappear not because they were forgotten, but because no usable
digital copy survives.

**afterprint** is a slow-built labor of love and a small contribution to
preserving lost media. It turns scans of rare and out-of-print books into
paragraph-aware text and optional local translations, so they can be read,
searched, and shared again.

Paragraphs stay intact through recognition and translation. The application is
open source, and the pipeline runs locally.

## Setup

On macOS:

```bash
brew install rust cmake tesseract tesseract-lang leptonica pkg-config

python3 -m venv .venv
source .venv/bin/activate
python -m pip install -U pip ctranslate2 huggingface_hub torch sentencepiece sacremoses "transformers<5"
```

## Use

OCR a folder of page images:

```bash
cargo run --release -- ./scans \
  --out ./out \
  --ocr-language eng
```

Add local translation with OPUS:

```bash
cargo run --release -- ./scans \
  --out ./out \
  --ocr-language ron \
  --translation-model opus \
  --translation-source ro \
  --translation-target en
```

## Models

- **OPUS** uses smaller language-specific or multilingual models. Choose it
  when your language pair is supported.
- **NLLB** uses one general multilingual model and supports any two different
  languages in afterprint's 93-language catalog.

On first use, afterprint asks before downloading and converting a model, then
caches it in `~/.config/afterprint/models`. Unsupported OPUS pairs stop before
anything is downloaded; use NLLB or pass a converted CTranslate2 model with
`--translation-model-path`.

Run `cargo run -- --help` for all languages, output modes, and custom-model
options.

## License

afterprint's source code is available under the [MIT License](LICENSE).
Translation models are downloaded separately and keep their own licenses:

- [NLLB-200 distilled 600M](https://huggingface.co/facebook/nllb-200-distilled-600M)
  is licensed under CC BY-NC 4.0. It does not permit commercial use, and Meta
  describes it as a research model not intended for production or document
  translation. Its integration here is experimental.
- OPUS model terms may vary by checkpoint. Review the selected Helsinki-NLP
  model card before use.
