# PM Brief — P1-1

## Goal

Generate opt-in, aligned source and translated PDFs from the canonical OCR
document, with one fixed A4 page per input image and Unicode text rendered by
embedded fonts. Docker uses Noto; native hosts use the first installed
Unicode-capable fallback.

## Deliverables

- `src/pdf.rs` renders shared source/translated PDF pages.
- `src/cli.rs` accepts `--pdf` and `--pdf-content filtered|raw` and removes
  the misleading unused PDF flags.
- `src/main.rs` writes `source.pdf` after OCR/filtering and `translated.pdf`
  after translation, preserving physical page indexes.
- `Dockerfile` installs the Noto font coverage required by the renderer.
- `README.md` documents the command and output contract.
- Renderer tests cover A4 geometry, raw-vs-filtered selection, metadata, blank
  failed pages, fitting, measured line wrapping, and Romanian Unicode font
  coverage.

## Non-goals

- PDF input, EPUB, bookmarks, chapter detection, manual cropping, images,
  tables, columns, footnotes, captions, or complex layout.
- A second PDF generator or a public document model. The existing transitive
  `lopdf` type is used only to encode the PDF catalog language string because
  `printpdf` does not expose that metadata field.
- Changing OCR, filtering, segmentation, or translation semantics from P0-1/
  P0-2.

## Contracts

- `--pdf` is opt-in; text-only runs keep current behavior.
- Default PDF content is filtered OCR; `--pdf-content raw` uses untouched raw
  page OCR text.
- `source.pdf` and `translated.pdf` contain exactly one A4 page for every
  input image, including blank pages for OCR failures.
- Page `N` in either PDF corresponds to input page `N`; no source image is
  embedded.
- `--title`, `--author`, and `--language` populate PDF metadata; `/Lang` uses
  the supplied language code.
- Fonts are embedded, with Docker-provided Noto Latin/Cyrillic/Greek, Arabic,
  and CJK fallback coverage. Native hosts use installed Arial/New York,
  Arabic, and CJK equivalents; missing coverage fails PDF generation clearly.
- Line wrapping uses embedded-font glyph advances, not a full-em estimate per
  character, so the text column uses the available A4 width.

## Invariants

- Physical indexes are never renumbered.
- Source and translated PDFs use the same page renderer and page count.
- Raw OCR remains available in the existing page artifacts.
- PDF generation does not run unless `--pdf` is supplied.
- Rendering never reads or embeds input images.
- Text is retained while font size is reduced to fit the fixed page; failed
  OCR pages remain blank.

## Acceptance matrix

| Requirement | Implementation | Evidence |
| --- | --- | --- |
| Opt-in CLI and content mode | `src/cli.rs`, `src/main.rs` | CLI/parser tests; `cargo test` |
| One A4 page per input page | `src/pdf.rs` | renderer page-count/geometry tests; `cargo test` |
| Source/translated alignment | `src/main.rs`, `src/pdf.rs` | shared-page tests; `cargo test` |
| Filtered default and raw override | `src/main.rs`, `src/pdf.rs` | content-selection test; `cargo test` |
| Failed OCR blank page | `src/pdf.rs` | failed-page test; `cargo test` |
| Metadata and no images | `src/pdf.rs` | PDF byte/catalog test; `cargo test` |
| Unicode font coverage | `Dockerfile`, `src/pdf.rs` | glyph-coverage test; rendered runtime QA |
| User-facing output contract | `README.md` | documentation review |

## Evaluation command

```bash
rtk cargo test
bash tests/launcher.sh
```

The renderer tests create PDFs from page records, inspect page geometry/count,
metadata, text selection, and embedded-font output without requiring
copyrighted scans or a translation model.

## Risks

- Font fallback is character-based and intentionally does not implement
  complex page layout or semantic shaping.
- Translated page text is already an approximate P0-2 mapping; P1-1 preserves
  that mapping exactly.
- Very dense OCR may use a small font to preserve one-page alignment.
- Native hosts without the Docker Noto packages must provide installed Unicode
  font fallbacks; Docker remains the supported path.

## Open CEO decisions

None. The PRD supplies the material behavior decisions for this issue.
