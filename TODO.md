# To-Dos

## Markdown Conversion

Context: our markdown conversion currently lives in `crates/core/src/markdown.rs`.

### Preserve Table Structure

- Add markdown conversion for simple `table` elements as pipe tables.
- Escape pipe characters inside table cells.
- Detect layout tables and unwrap their cell content instead of emitting malformed data tables.
- Preserve complex tables with `rowspan` or `colspan` as cleaned HTML instead of flattening them.
- Current gap: tables fall through to generic child rendering and can collapse to concatenated text such as `ABx|yz`.

### Convert Footnotes And Citations

- Convert inline footnote references to `[^n]`.
- Emit footnote definitions at the end of markdown.
- Remove backlink-only anchors from footnote bodies.
- Support common variants: `fnref:`, MediaWiki cite refs, Google Docs/Word-style footnotes, loose footnote sections, and sidenotes where practical.
- Current gap: footnote refs render as ordinary links/text, for example `See[1](#fn:1)`.

### Preserve Math

- Convert MathML, KaTeX, and MediaWiki math forms to inline `$...$` or block `$$...$$` markdown.
- Extract LaTeX from `data-latex`, `alttext`, and KaTeX annotations.
- Handle equation tables without flattening equation content.
- Current gap: math nodes generally collapse to plain text or disappear.

### Improve Code Block Normalization

- Preserve fenced-code language identifiers from `data-lang`, `data-language`, `language-*`, and common highlighter classes.
- Strip copy buttons, toolbars, line numbers, gutter cells, and syntax-highlighter UI chrome.
- Normalize common code block shapes used by Prism, Pygments, Rouge, Chroma, React Syntax Highlighter, CodeMirror, WordPress, Mintlify, and Writerside where feasible.
- Current gap: code blocks emit untyped fences and can include highlighter noise.

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
  - `codeblocks--mintlify`
  - `codeblocks--chatgpt-codemirror`
