# To-Dos

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
