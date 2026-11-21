# Differential fuzzing: prettier-js-rs against Prettier 3.6.2

Generates inputs beyond the recorded corpus, formats each with the port and with Prettier, and
reports every input where they disagree. Numbers from past runs are in `results.md`; minimized
repros are lines in `../cases.jsonl`.

## What it checks

Every input gets one category:

| category | meaning |
|---|---|
| `equal` | both formatted it, same bytes |
| `MISMATCH` | both formatted it, different bytes |
| `SOUNDNESS` | the port formatted it, Prettier threw |
| `refused` | the port returned `Unsupported` (the front door then runs Prettier), Prettier formatted it |
| `refused_opts` | `Options::from_cli` refused the flags |
| `both_error` | the port refused, Prettier threw |
| `RUST_PANIC` / `RUST_CRASH` / `RUST_TIMEOUT` | the port panicked, aborted, or ran over the time limit |
| `prettier_timeout` | the port formatted it, Prettier hung or ran out of memory |

`MISMATCH`, `SOUNDNESS` and the `RUST_*` categories are findings. Refusals are not, but they are
counted, since each one costs a fallback to Prettier.

When the port formats an input, the driver also formats the output again and records an
idempotence class:

| class | meaning |
|---|---|
| `idem_ok` | `rust(rust(x)) == rust(x)` |
| `nonidem_same_as_prettier` | Prettier is not idempotent there either, and both second passes agree (expected) |
| `nonidem_diverges` | first outputs agree, second passes differ: a mismatch on input `prettier(x)` (a finding) |
| `nonidem_in_other` | not idempotent on an input that already has a finding |
| `idem_second_refused` / `idem_second_panic` | the port refuses, or panics on, its own output |

## Pieces

| path | what it is |
|---|---|
| `oracle.mjs` | Long-lived Node process running `prettier.format` in-process. Reads JSON lines `{id, args, text}`, where `args` are Prettier CLI flags as in the corpus, and turns them into the options object the CLI passes to `format`. |
| `driver/` | The Rust driver, `equiv-fuzz`. Each worker owns one oracle and one `rust-worker` child (the same binary calling `prettier_js_rs::format`), so hangs, aborts and stack overflows on either side are caught by a time limit and the child is restarted. |
| `gen/` | Generators. Each writes `{name, args, stdin}` lines to stdout. |
| `libfuzzer/` | cargo-fuzz target that grows a corpus by the port's coverage. `header.rs` is the byte layout it shares with the driver: byte 0 picks the file type, bytes 1-2 the options, the rest is UTF-8 text. |
| `select.mjs` | Picks findings to minimize: up to K per (category, idempotence class, language), shortest first; soundness findings are also split by Prettier's error message. |
| `triage.mjs` | Sorts mismatches by known cause by re-running variants (`es5` changed to `all`, comments blanked, a `.js` file renamed `.ts`), so minimization can go to the unexplained ones. |
| `save-case.mjs` | Appends a minimized finding to `../cases.jsonl`. |
| `report.mjs` | Prints the per-generator, per-language table of a run, with the 95% upper bound 3/N on the mismatch rate where a cell had no finding (N = inputs both sides formatted). |

### Generators

| generator | what it makes |
|---|---|
| `gen/splice-js.mjs` | JS/TS/JSX: parses the corpus fixtures with Babel and swaps subtrees of the same kind between files (expressions, statements, TS types, JSX children and attributes, class/object/interface members, parameters), and inserts comments at token boundaries. Mutants that no longer parse are dropped. |
| `gen/splice-css.mjs` | CSS/SCSS/Less: with the PostCSS parsers Prettier uses, swaps rules, at-rules, declarations, comments, values, selectors and at-rule params between fixtures of the same language. |
| `gen/graphql.mjs` | GraphQL: token-level edits (comments at token boundaries, string escapes, block strings, swapped token runs, BOM and CRLF). |
| `gen/options.mjs` | Random values for every option the port reads (printWidth 0-1000, tabWidth 0-8, every boolean and enum, end-of-line, a forced style `--parser`), applied to the corpus or to another generator's output. |

`--feedback` on `equiv-fuzz run` also runs each input's Prettier output as a new input, counted
under `<gen>+fb`.

## Setup

Requirements:

- A Prettier 3.6.2 checkout, built, with its `node_modules` installed (the generators load
  `@babel/parser`, `postcss`, `postcss-scss`, `postcss-less` and `graphql` from it).
- The seed corpus: `corpus/inputs.jsonl`, one `{name, args, stdin}` line per recorded case.

Both are found under one directory, `$PRETTIER_EQUIV`, laid out as `prettier/` (the checkout) and
`corpus/inputs.jsonl`. `$PRETTIER_DIST` can point the oracle at a different built
`dist/prettier`.

```sh
export PRETTIER_EQUIV=/path/to/dir                # contains prettier/ and corpus/inputs.jsonl
export PRETTIER_DIST=$PRETTIER_EQUIV/prettier/dist/prettier
F=$PWD                                            # this folder
export CARGO_TARGET_DIR=$PRETTIER_EQUIV/target-fuzz
(cd $F/driver && cargo build --release)
export B=$CARGO_TARGET_DIR/release/equiv-fuzz     # also read by triage.mjs
cd $PRETTIER_EQUIV
```

`equiv-fuzz run` takes `--workers N` (default 4), `--limit N`, `--feedback`, and
`--log-all FILE` (one `{name, category, idem}` line per input). `FUZZ_TIMEOUT` sets the per-call
limit in seconds (default 20). Each run writes `summary.json` (counts) and `findings.jsonl` (every
finding, with both outputs) under `--out`.

## Running the streams

Baseline: the seed corpus itself must give no findings.

```sh
$B run --gen corpus --in corpus/inputs.jsonl --out fuzz-runs/corpus
```

Syntax-tree splicing, then rule/declaration/value splicing:

```sh
node $F/gen/splice-js.mjs --n 300000 --seed 7 | $B run --gen splice-js --in - --out fuzz-runs/splice-js
node $F/gen/splice-css.mjs --n 250000 --seed 11 | $B run --gen splice-css --in - --out fuzz-runs/splice-css
```

GraphQL token edits:

```sh
node $F/gen/graphql.mjs --n 150000 --seed 21 | $B run --gen graphql-tokens --in - --out fuzz-runs/graphql-tokens --feedback
```

Random options, on the corpus and on spliced inputs:

```sh
node $F/gen/options.mjs --per 20 --seed 1 | $B run --gen options-corpus --in - --out fuzz-runs/options-corpus
node $F/gen/splice-js.mjs --n 150000 --seed 9 | node $F/gen/options.mjs --in - --per 1 --seed 3 \
  | $B run --gen options-splice-js --in - --out fuzz-runs/options-splice-js
node $F/gen/splice-css.mjs --n 100000 --seed 13 | node $F/gen/options.mjs --in - --per 1 --seed 4 \
  | $B run --gen options-splice-css --in - --out fuzz-runs/options-splice-css
```

Prettier's output fed back as input:

```sh
$B run --gen corpus --in corpus/inputs.jsonl --out fuzz-runs/feedback-corpus --feedback
node $F/gen/options.mjs --per 5 --seed 2 | $B run --gen options --in - --out fuzz-runs/feedback-options --feedback
node $F/gen/splice-css.mjs --n 60000 --seed 12 | $B run --gen splice-css --in - --out fuzz-runs/feedback-splice-css --feedback
node $F/gen/splice-js.mjs --n 100000 --seed 8 | $B run --gen splice-js --in - --out fuzz-runs/feedback-splice-js --feedback
```

Fresh inputs: the same commands with seeds no fix has been checked against.

Coverage-guided growth with libFuzzer (nightly toolchain and `cargo-fuzz`), then the grown corpus
against Prettier:

```sh
$B encode-seeds corpus/inputs.jsonl fuzz-runs/libfuzzer/seeds
cargo +nightly fuzz build --fuzz-dir $F/libfuzzer -s none -O format
$CARGO_TARGET_DIR/<host-triple>/release/format -fork=4 -ignore_crashes=1 -ignore_timeouts=1 \
  -ignore_ooms=1 -timeout=10 -rss_limit_mb=4096 -max_len=8192 -max_total_time=3600 \
  -artifact_prefix=fuzz-runs/libfuzzer/artifacts/ fuzz-runs/libfuzzer/corpus fuzz-runs/libfuzzer/seeds
$B decode-corpus fuzz-runs/libfuzzer/corpus > fuzz-runs/libfuzzer.jsonl
$B run --gen libfuzzer --in fuzz-runs/libfuzzer.jsonl --out fuzz-runs/libfuzzer-diff --feedback
```

The target catches the port's panics so growth continues; the differential run reports them.
`timeout-*` artifacts are inputs where the port ran past 10 s; run them through `$B run` with a
larger `FUZZ_TIMEOUT` to see whether Prettier hangs too.

Tabulate one or more runs:

```sh
node $F/report.mjs fuzz-runs/splice-js fuzz-runs/options-corpus
```

## Minimizing and saving a finding

1. Pick findings: `node $F/select.mjs fuzz-runs/splice-js/findings.jsonl --k 10 > picked.jsonl`.
   For a large run, first `node $F/triage.mjs fuzz-runs/splice-js/findings.jsonl triage/` (set
   `B` to the driver) and pick from `triage/unexplained.jsonl`.
2. Minimize: `$B minimize --in picked.jsonl --out min/`. It deletes lines, then characters, then
   option flags, keeping the finding's category. A soundness finding also keeps Prettier's error
   message. A panic keeps its message, with numbers stripped, and whether Prettier succeeded.
   Results go to `min/minimized.jsonl`.
3. Look at one input by hand: `$B one FILE --stdin-filepath x.ts [flags]` prints both outputs.
4. Save one case per root cause: `node $F/save-case.mjs min/minimized.jsonl INDEX NAME`. It
   appends `{name, args, stdin}` to `../cases.jsonl` and prints both outputs. For a
   `nonidem_diverges` finding it saves the first output, the input on which the two sides
   differ.
