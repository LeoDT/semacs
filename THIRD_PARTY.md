# Third-party source

## Bundled default font

- Lilex, Copyright 2019 The Lilex Project Authors
  (<https://github.com/mishamyrt/Lilex>), under SIL Open Font License 1.1.
- `assets/fonts/lilex/{Lilex-Regular,Lilex-Bold,Lilex-Italic,Lilex-BoldItalic}.ttf`
  and `assets/fonts/lilex/OFL.txt` are copied verbatim from the same paths in
  Zed snapshot `ba63a022c9a22ca66d68ef2a8f0bb9b5b0a9984d`. No font data or
  names are modified. This is an embedded font asset, not a system font
  installation or a dependency on the local Zed checkout.
- The four font files are embedded in semacs; retain their copyright and
  the complete `assets/fonts/lilex/OFL.txt` license when distributing binaries.
  The fonts remain under OFL; that license does not apply to documents created
  with them. Font files may not be sold by themselves.

## Completion behavior reference and matching dependency

- Vertico (`https://github.com/minad/vertico`, local snapshot `c60d802`) and
  `extensions/vertico-directory.el` were read as behavior references for candidate
  insertion, acceptance, raw input and directory navigation. No Emacs Lisp
  implementation was copied or translated.
- `nucleo-matcher` 0.3.1 is a registry dependency under MPL-2.0. Its manifest and
  license were reviewed; no source is vendored or patched. Include its license
  and covered-source availability in the dependency audit before distributing
  binaries. semacs uses the matcher, not nucleo's picker runtime or query syntax.

## Syntax parsing and paint decoration dependencies

- `tree-sitter` 0.27.1 and `tree-sitter-language` 0.1.9 are registry
  dependencies under MIT, attributed to Max Brunsfeld. `tree-sitter-rust`
  0.24.2 is a registry dependency under MIT, attributed to Maxim Sokolov.
  Their manifests and license texts were reviewed; no parser or grammar source
  is vendored or patched.
- semacs consumes `tree-sitter-rust::HIGHLIGHTS_QUERY` from the dependency and
  maps its captures to neutral syntax styles. `src/syntax/rust.scm` adds local
  captures for constants, numbers, operators and punctuation; no Zed language
  or project subsystem is imported.
- E2 additionally consumes registry grammars and their bundled highlight queries:
  `tree-sitter-javascript` 0.25.0 (MIT, Copyright 2014 Max Brunsfeld),
  `tree-sitter-typescript` 0.23.2 (MIT, Copyright 2017 Max Brunsfeld),
  `tree-sitter-html` 0.23.2 (MIT, Copyright 2014 Max Brunsfeld),
  `tree-sitter-css` 0.25.0 (MIT, Copyright 2018 Max Brunsfeld), and
  `tree-sitter-md` 0.5.4 (MIT, Copyright 2021 Matthias Deiml).
  JavaScript and CSS license texts were reviewed in the registry package.
  The other three packages omit their top-level license file; their MIT
  notices were reviewed at the exact upstream commits identified by
  `.cargo_vcs_info.json`:
  [TypeScript](https://github.com/tree-sitter/tree-sitter-typescript/blob/f975a621f4e7f532fe322e13c4f79495e0a7b2e7/LICENSE),
  [HTML](https://github.com/tree-sitter/tree-sitter-html/blob/5a5ca8551a179998360b4a4ca2c0f366a35acc03/LICENSE),
  [Markdown](https://github.com/tree-sitter-grammars/tree-sitter-markdown/blob/41ff24a6c775492c543b09228aeb98d1cd2b3a0f/LICENSE).
  Markdown's highlight queries credit nvim-treesitter; retain that upstream
  attribution in the distribution dependency audit.
- E2 uses HTML's bundled script / style injection query.
  `src/syntax/markdown-injections.scm` independently declares the selected
  block-to-inline and fenced-code captures; other upstream injection rules
  are not imported. No parser, scanner, wrapper implementation or Zed
  language subsystem is vendored or translated.
- The transitive `streaming-iterator` 0.1.9 is MIT OR Apache-2.0, attributed
  to Steven Fackler. The existing transitive `smallvec` 1.16.4 (MIT OR
  Apache-2.0, The Servo Project Developers) is now also a direct dependency
  for inline paint decorations. Their manifests and MIT notices were reviewed.
- Include these dependency notices, including the bundled grammar query, in
  the dependency audit before distributing binaries.

## JavaScript runtime dependency

- `rquickjs`, `rquickjs-core` and `rquickjs-sys` 0.14.0 are registry
  dependencies under MIT; manifests and the top-level license were reviewed.
  Attribution includes Mees Delzenne and Rquickjs Contributors.
- `rquickjs-sys` includes upstream QuickJS-NG C sources under MIT, attributed
  in its license to Fabrice Bellard, Charlie Gordon, Ben Noordhuis and
  Saúl Ibarra Corretgé. semacs does not vendor or patch those sources and
  does not install QuickJS's standalone `std` / `os` modules.
- Include runtime / engine notices and transitive dependency licenses in the
  dependency audit before distributing binaries.

## Zed text components

- Repository: <https://github.com/zed-industries/zed>
- Snapshot: `ba63a022c9a22ca66d68ef2a8f0bb9b5b0a9984d`
- Copyright / attribution: Zed Industries, Inc. and contributors. Upstream
  `LICENSE-APACHE` carries `Copyright 2022 - 2025 Zed Industries, Inc.` and is
  preserved verbatim here; `LICENSE-GPL` is also copied verbatim.
- Imported and locally modified: 2026-10-31.

| Upstream path | Local path | License |
|---|---|---|
| `crates/rope/src/rope.rs` | `src/text/rope/mod.rs` | GPL-3.0-or-later |
| `crates/rope/src/{chunk,offset_utf16,point,point_utf16,unclipped}.rs` | Same basenames under `src/text/rope/` | GPL-3.0-or-later |
| `crates/sum_tree/src/sum_tree.rs` | `src/text/sum_tree/mod.rs` | Apache-2.0 |
| `crates/sum_tree/src/{cursor,tree_map,property_test}.rs` | Same basenames under `src/text/sum_tree/` | Apache-2.0 |
| `crates/gpui_util/src/lib.rs`: `is_utf8_char_boundary`, `debug_panic!` | `src/text/util.rs` | Apache-2.0 |
| `crates/util/src/util.rs`: `RandomCharIter` | `src/text/test_support.rs` | Apache-2.0 |

The licenses were checked against each owning crate's manifest and license
file at this snapshot. No additional NOTICE file accompanies this imported
source in that snapshot. Repository license texts and copyright notices are
preserved as `LICENSE-GPL` and `LICENSE-APACHE`.

### Local adaptations

- Integrated into one crate using module-relative paths, without Zed workspace,
  language, project, text::Buffer, editor or runtime dependencies.
- Removed Zed tracing attributes and test logger constructors; retained log
  calls, core Rope / SumTree algorithms, TreeMap / TreeSet and property strategy.
- Retained all source unit-test bodies. Replaced GPUI RNG test attributes with
  pure Rust test wrappers and an explicit seeded runner (same default iteration
  counts, `SEED` / `ITERATIONS` overrides). This does not start a GUI. Removed
  unused `RandomCharIter::with_simple_text`; kept `SIMPLE_TEXT` support.
- Simplified test-only small chunk / tree configurations to `cfg(test)`.
- The helper macro is module-local rather than exported from the crate root.
- Added SPDX and modification notices; applied Rust formatting. Converted
  `ChunkSlice -> Chunk` from Into to From; disambiguated one test's Ord::cmp
  after crate integration; retained the lending Lines::next API with a scoped
  Clippy allowance (its return borrows the line buffer).
- Upstream benchmark harness was not imported.

These files contain copied / adapted implementation, not merely design
references. Other semacs modules are independently implemented unless explicitly
listed here. GPUI is consumed through the existing gpui-kit dependency, not
copied as a separate runtime. See `LICENSE` for combined-work distribution and
corresponding-source requirements. Dependency license notices must additionally
be audited before distributing binaries; this inventory covers vendored source.

### Optional native profiling dependency

The opt-in Cargo feature `native-profiler` enables the existing GPUI profiler
through `gpui-kit/profiler`. It adds the registry dependency `hdrhistogram`
7.6.0 (manifest license `MIT/Apache-2.0`); no dependency source was vendored or
patched. Its notices must be included in the dependency audit if distributing
a binary built with this feature. The default product does not enable it.

### Input behavior reference

The frozen editor `crates/editor/src/input.rs` and Linux GPUI input paths were
read to compare marked-text replacement, unmarking, and cancellation semantics.
semacs retains its own separate preedit projection and target-bound adapter;
it does not copy Zed's editor transactions, multi-cursor implementation, or
Linux platform backend, and introduces no GPUI patch.

### Tests

`cargo test --all-targets` runs the retained Rope, Chunk, SumTree and TreeMap /
TreeSet tests alongside semacs tests. For an extended deterministic run:

```sh
SEED=1000 ITERATIONS=1000 OPERATIONS=100 cargo test --lib text::
```

No future import of wrap maps, editor element or input algorithms is implied by
this inventory; any copied implementation must be recorded with its import.
