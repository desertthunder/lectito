# To-Dos

## Extraction Quality

Context: current extraction is strong for many article pages, but the remaining
edge cases usually fall into wrong-root selection, over-included chrome, or
metadata/header cleanup.

### Retry Short Or Suspicious Extractions

- If extracted text is far below the page's best content signals, retry with relaxed removal settings.
- Retry without unlikely-candidate stripping when the first result is under a useful word threshold.
- Retry with hidden-element removal disabled when the first result is extremely short.
- Prefer a larger focused subtree when the current result is only notes, metadata, or a single step.

### Clean Reference Site Chrome

- Remove skip links, "from Wikipedia" boilerplate, edit links, table-of-contents blocks, and infoboxes when extracting reference pages.
- Preserve equations, tables, footnotes, and citation references while removing navigation chrome.
- Remove heading permalink/edit anchors but keep the heading text.

## Markdown Conversion

Context: our markdown conversion currently lives in `crates/core/src/markdown.rs`.

### Preserve Inline Semantic Elements

- Convert `mark` to `==highlight==`.
- Convert `del`, `s`, and `strike` to `~~strikethrough~~`.
- Preserve or intentionally handle `sup`, `sub`, `iframe`, `video`, `audio`, `svg`, and `math`.
- Current gap: these elements usually render as plain child text, losing semantic meaning.

### Markdown Cleanup Edge Cases

- Strip `<wbr>` without introducing spaces.
- Remove empty links like `[](url)` while preserving images.
- Add a space between sentence exclamation marks and image markdown so `Yey!![img]` does not become ambiguous markdown.
- Continue removing duplicate leading title headings before markdown output.

### Expand Test Coverage

- Add focused Rust tests in `crates/core/src/markdown.rs` for each feature class above.
- Add representative fixtures before broad implementation:
  - `elements--data-table`
  - `elements--complex-tables`
  - `elements--srcset-normalization`
  - `elements--embedded-videos`
  - `math--katex`
  - `math--mathjax-svg`
  - `footnotes--numeric-anchor-id`
  - `footnotes--google-docs-ftnt`
