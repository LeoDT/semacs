# Third-party source

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
