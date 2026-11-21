# Doc printer equivalence

Compares the Rust doc printer (`doc-printer-rs/src/port.rs`) with Prettier 3.6.2's
`printDocToString` and `getStringWidth`, in-process in Node, on every doc up to a size bound.

## What it proves

For every doc of size ≤ 5 over the `native` alphabet below, under every option set listed in
[Options](#options), the port's result equals Prettier's: the printed text, the cursor fields, and
the error message when both throw. Size counts constructors: each leaf, wrapper and list is 1, and
a list adds its items. A divergence that needs a doc of size 6 or more, or a doc command outside
the alphabet, is not covered. Random docs of up to 300 nodes (`fuzz.mjs`) probe past the bound
without proving anything.

`getStringWidth` is proved on every Unicode scalar value.

## Alphabets

- **`native`** (the main check, `harness/src/native.rs`). Docs are Rust `Doc` values, the
  representation the port's printers hand to the printer, printed with no parsing step. Each is
  also written out as Prettier's doc, with a `Shared` node becoming one JS object reached from
  every place it occurs, and Node prints that with Prettier. Contents:
  - leaves: `""`, `"a"`, `" "`, `"\t"`, `"字"`, line, softline, hardline and literalline (raw and
    with break-parent), break-parent, trim, cursor, line-suffix-boundary, `fill([])`;
  - wrappers: indent; align `2`, `-1`, `"> "`, `""`, `" "`, `0`, dedentToRoot, markAsRoot;
    group × {break, no break} × {no id, id A, id B}; line-suffix; label; indent-if-break ×
    {id A, id B, none} × {negate, not}; a `Shared` node; one `Shared` node used twice;
  - two children: if-break × {no id, A, B}; a group whose `contents` is not its first expanded
    state (what `strip_trailing_hardline` builds);
  - lists: concat (≥ 2 items), fill (≥ 0), conditionalGroup (≥ 1 state; plain, `break`, id A),
    whose contents is the same node as its first state.
- **`full`**, **`small`**, **`exotic`** (`grammar.mjs`). Docs are JSON, read through the port's
  `from_value`. `full` is the JSON form of what Prettier's builders make; `small` is a subset
  that reaches size 6; `exotic` holds what no builder makes: `expandedStates: []`, invalid docs
  (`null`, `1`, `{type: "bogus"}`), `[]`, align `0` / `1.5` / `""`.

Why `native` exists: the JSON runs once passed while the YAML and Markdown printers indented for
`align("")`. `from_value` reads `""` as a no-op, but the printers build `Align::Text("")`
directly, which no JSON doc reaches.

Group ids only matter through equality, so only docs whose first id in preorder is `A` are
checked.

### Options

printWidth ∈ {0, 1, 2, 3, 4, 5, 6, 8, 12, 80, Infinity}. Docs with indent, align or
indent-if-break take each width with (useTabs, tabWidth) ∈ {false, true} × {2, 4}, plus
`{printWidth 4, crlf}` and `{printWidth 4, useTabs, tabWidth 4, cr}`. Other docs take useTabs
false and tabWidth 2 at each width, plus `{printWidth 4, crlf}`; both printers read useTabs and
tabWidth only when building an indentation.

## Running

```sh
cd demo/real-world/prettier-rs/equiv/doc-printer

# Prettier 3.6.2: install it here, or point PRETTIER_DIR at a package directory
npm install --prefix . --no-save prettier@3.6.2   # or: export PRETTIER_DIR=/path/to/prettier

# the napi harness (on Linux the library is libdoc_printer_equiv.so)
(cd harness && cargo build --release)
cp "${CARGO_TARGET_DIR:-harness/target}/release/libdoc_printer_equiv.dylib" harness/doc_printer_equiv.node

node width.mjs              # getStringWidth: every scalar value, ~1M structured strings
node lone-surrogates.mjs    # strings with lone surrogates, through the printer

# exhaustive runs, sharded over 3 processes
for k in 0 1 2; do node native-run.mjs --min 1 --max 5 --worker $k --workers 3 & done; wait
for k in 0 1 2; do node run.mjs --alphabet full   --min 1 --max 5 --worker $k --workers 3 & done; wait
for k in 0 1 2; do node run.mjs --alphabet small  --min 1 --max 6 --worker $k --workers 3 & done; wait
for k in 0 1 2; do node run.mjs --alphabet exotic --min 1 --max 5 --worker $k --workers 3 & done; wait

for s in 1 2 3; do node fuzz.mjs --seconds 180 --seed $s & done; wait   # random docs

node minimize.mjs out/mismatch-exotic-1-5-w0of3.jsonl 1 repros/name.json   # shrink a JSON-alphabet mismatch
```

Each run writes `out/summary-….json` and up to 2,000 mismatch lines per kind to
`out/mismatch-….jsonl`. `native-run.mjs --round-trip` also lists native docs that `from_value`
does not read back unchanged; every one holds `align("")` or a shared node.

| File | Role |
| --- | --- |
| `harness/` | napi crate over the port: JSON docs (`printMany`, `printBatch`), widths, native docs (`nativeCount`, `nativeBatch`, `nativeRoundTrip`). |
| `lib.mjs` | Loads Prettier and the harness. |
| `grammar.mjs`, `compare.mjs` | JSON alphabets and enumerator; option sets and comparison. |
| `run.mjs`, `native-run.mjs` | Exhaustive runs over a JSON alphabet / the native alphabet. |
| `width.mjs`, `lone-surrogates.mjs` | `getStringWidth` checks. |
| `fuzz.mjs`, `minimize.mjs` | Random docs; mismatch shrinker. |
| `repros/` | Five hand-built docs that once diverged (empty `expandedStates`, a conditional group whose contents is not its first state, a lone surrogate next to U+10FC00). The port now matches Prettier on all five. |

## Current counts

Apple Silicon, 12 cores, 3 workers. Every row has 0 mismatches.

| Check | Docs | Prints |
| --- | ---: | ---: |
| `native`, sizes 1–5 (about 8 min) | 19,398,096 | 742.2M |
| `full`, sizes 1–5 | 10,959,504 | 397.1M |
| `small`, sizes 1–6 | 5,934,546 | 178.6M |
| `exotic`, sizes 1–5 | 708,950 | 21.7M |
| random, 3 × 180 s, ≤ 300 nodes | 3,730,800 | 22.4M |
| `getStringWidth` | 1,112,064 scalars, 1,060,008 strings | |
| lone surrogates | 864 strings | 6,912 |
