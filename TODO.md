# To-Dos

## Markdown Conversion

Context: our markdown conversion currently lives in `crates/core/src/markdown.rs`.

### Improve Image And Media Markdown

- Prefer the best image candidate from `srcset`, including URLs with commas in CDN paths.
- Preserve image `title` attributes in markdown image syntax.
- Convert figures to image markdown followed by caption text, while avoiding content-wrapper figures.
- Normalize `picture`, lazy-loaded images, placeholder `data:` images, `data-src`, and `data-srcset` before markdown conversion.
- Convert YouTube and X/Twitter embeds to Obsidian-style embed links.
- Current gap: images use only `src` and `alt`; local testing chose `small.jpg` over a larger `srcset` candidate.

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
