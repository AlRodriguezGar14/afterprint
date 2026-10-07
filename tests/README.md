# Regression and scan evaluation

From the repository root, run the deterministic record tests and launcher
forwarding checks:

```bash
rtk cargo test
rtk bash tests/launcher.sh
```

The Rust regression uses synthetic TSV records, independent of Tesseract
version. `fixtures/cli-scans/` has blank PGM images on pages 1 and 3 and an
invalid PNG on page 2. Run it to check CLI page identity and PDF alignment:

```bash
EVAL_ROOT="$(mktemp -d)"
./afterprint tests/fixtures/cli-scans --out "$EVAL_ROOT/cli" --pdf
```

The run is expected to exit nonzero after writing its artifacts. Confirm
`debug/processing.json` reports page 2, numbered text outputs exist for pages
1–3, and `source.pdf` has three pages with page 2 blank. With a prepared local
model, repeat with translation enabled and confirm `translated.pdf` also has
three pages and a blank page 2:

```bash
EVAL_ROOT="$(mktemp -d)"
./afterprint tests/fixtures/cli-scans --out "$EVAL_ROOT/translated" --pdf \
  --translation-model opus --translation-source ro --translation-target en
```

For content evaluation, use a real scan folder and matching language codes.
In one shell session, compare the scans with raw and filtered text, review
`debug/exclusions.json`, then compare the source and translated PDFs with
their page-addressable text outputs:

```bash
SCANS="/path/to/book scans"
EVAL_ROOT="$(mktemp -d)"
./afterprint "$SCANS" --out "$EVAL_ROOT/filtered" --ocr-language ron --pdf
./afterprint "$SCANS" --out "$EVAL_ROOT/raw" --ocr-language ron --pdf --pdf-content raw
./afterprint "$SCANS" --out "$EVAL_ROOT/translated" --ocr-language ron --pdf \
  --translation-model opus --translation-source ro --translation-target en
```

Translation requires a prepared model. Compare `translations/` and
`debug/segments.json` with `translated.pdf`; page splits are deterministic
approximations at word boundaries and may fall inside sentences. For Unicode
scans, check Latin, Cyrillic, Arabic, and CJK glyphs and clipping in a PDF
viewer.
