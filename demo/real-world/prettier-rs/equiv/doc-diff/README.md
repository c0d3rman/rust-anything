# Doc-level differential check

This check compares the Doc that the port builds with the Doc that Prettier 3.6.2 builds for the
same input and options, before either one is printed. The port's doc comes from
`prettier_js_rs::text_to_doc` and Prettier's from `prettier.__debug.printToDoc`. Both docs are
taken after `propagateBreaks`.

The doc printer is checked on its own in `../doc-printer`. Together, equal docs and equivalent
printers mean equal output at every print width that leaves the doc unchanged. That is why output
strings at a few recorded widths are not enough.

## Files

| File | What it does |
| --- | --- |
| `rs/` | `doc-diff-rs`, a long-lived server around the port. It reads one JSON request per line (`args`, `stdin`, optional `width`) and answers with the port's doc as Prettier wire JSON, its `format` output, or both. A shared subtree is written once as `$def` and then as `$ref`. |
| `lib.mjs` | CLI flags to Prettier options, Prettier's `propagateBreaks`, rebuilding the port's doc with its sharing, and the canonicaliser `Canon`. |
| `driver.mjs` | `base` compares both docs and both final strings at each case's own options. `sweep` compares both docs over a list of widths, and both strings at every width from 0 to 200 when a doc changes with width. Runs in a worker with a 256 MB stack. |
| `summarize.mjs` | Tallies driver output per language and class. |
| `selftest.mjs` | Positive and negative controls for the canonicaliser, run on Prettier's own docs. |
| `show.mjs` | One input: both canonical docs, the first place they differ, and both strings. |
| `gen-probes.mjs` | Inputs aimed at each width- or tab-dependent construction site, with lengths and tab settings on both sides of each threshold. |

The inputs are JSONL, one case per line as `{name, args, stdin}`. `args` are Prettier CLI flags,
including `--stdin-filepath`. The recorded corpus and `../cases.jsonl` both use this format.

## Running

Three environment variables are needed. The scripts stop with a message if one is missing.

| Variable | Points to |
| --- | --- |
| `PRETTIER_ROOT` | `<prettier clone>`, a clone of Prettier 3.6.2 built with `yarn build` (the scripts load `dist/prettier/index.mjs`). |
| `CARGO_TARGET_DIR` | `<target dir>`, the directory `rs/` is built into. It is read both by the build and by the scripts, which start `<target dir>/release/doc-diff-rs`. |
| `DOC_DIFF_INPUTS` | Only for `show.mjs --case` when `--inputs` is not given: the inputs file holding that case. |

```sh
cd demo/real-world/prettier-rs/equiv/doc-diff
export PRETTIER_ROOT=<prettier clone>
export CARGO_TARGET_DIR=<target dir>
(cd rs && cargo build --release -j 3)
I=<corpus inputs.jsonl>          # the recorded corpus, or ../cases.jsonl
mkdir -p out

# Self-test: every 37th case of $I. Expect posFail: 0 and negFail: 0.
node selftest.mjs $I 37

# Corpus comparison at each case's own options, in 3 shards.
for s in 0 1 2; do node driver.mjs base $I out/base-$s.jsonl $s/3 & done; wait
node summarize.mjs base out/base-*.jsonl

# Width sweep.
for s in 0 1 2; do node driver.mjs sweep $I out/sweep-$s.jsonl $s/3 & done; wait
node summarize.mjs sweep out/sweep-*.jsonl
node gen-probes.mjs > out/probes.jsonl
for s in 0 1 2; do node driver.mjs sweep out/probes.jsonl out/probes-$s.jsonl $s/3 & done; wait
node summarize.mjs sweep out/probes-*.jsonl

# Inspect one case.
node show.mjs --case <name> --inputs $I [--width N] [--rebind]
```

On an M4, `base` over the corpus takes about 3 minutes; the full sweep takes about 75.

Classes in the `summarize.mjs` output:

- **doc-equal**: both docs are equal.
- **LATENT**: the docs differ but the strings are equal. A different width or option could expose
  a byte difference, so these matter most.
- **STR-DIFF**: the strings differ.
- **port-declines**: the port returned `Unsupported`.

### Which widths the sweep uses

Doc construction reads `printWidth` in only two places:

- **JS** `isLoneShortArgument` (`language-js/utils/index.js`): an argument is short when its length
  is at most `printWidth * 0.25`, or `- 2` for a zero-argument call. Lengths are integers, so the
  doc can change only at widths that are multiples of 4. JS, TS and Markdown with a code fence are
  therefore swept at 0, 4, ..., 200 and 1e6.
- **Markdown table cells** (`language-markdown/print/table.js`), printed with `printDocToString` at
  the real width. Markdown containing a `|` is swept at every width from 0 to 200.

Every other language reads no width while building the doc, and is checked at 17 widths from 0 to
1e6 to confirm that. Running the probes with `SWEEP_ALL=1` checks every width from 0 to 200 and
records where Prettier's doc changes, in `pChangesAt`.

`tabWidth` and `useTabs` are read in more places: ternaries, template-literal indentation, member
chains, short object keys, Markdown list alignment and YAML aligns. Those docs are compared at each
case's own tab settings, and `gen-probes.mjs` covers tabWidth 1 to 8, with and without tabs.

## Normalisations

Both docs go through one hash-consing canonicaliser (`Canon` in `lib.mjs`) and are equal when their
root numbers match. Each normalisation is justified by `src/document/printer.js`, where both forms
behave the same in `printDocToString` and in `fits`:

1. **Shared subtrees are expanded.** The printer never compares identity, and `propagateBreaks`
   reads a group that it has already visited through the flag it set on the first visit.
2. **Nested arrays are flattened, except a fill's own parts.** An array only pushes its items in
   order. Fill parts keep their positions.
3. **Labels are unwrapped.** `DOC_TYPE_LABEL` pushes `doc.contents`.
4. **`break-parent` is dropped.** After `propagateBreaks`, the printer treats it as a no-op and
   `fits` has no case for it. Running `propagateBreaks` again at print time is idempotent.
5. **Empty strings are dropped and adjacent strings are merged.** A merge happens only when
   `getStringWidth(a) + getStringWidth(b) === getStringWidth(a + b)`. Widths add string by string,
   and `trim` works across separate strings.
6. **`group.break` is compared by truthiness.** The printer reads only truthiness, so `true` and
   `"propagated"` are the same.
7. **A missing `if-break` branch counts as `""`.** Both printers test `if (contents)` before pushing.
8. **Group ids are renumbered by first appearance, and unreferenced ids are dropped.**
   `groupModeMap` is read only through the `groupId` of `if-break` and `indent-if-break`.

An earlier normalisation that unwrapped `align(0)` and `align("")` was removed. It hid the port's
printer handling `align("")` differently from Prettier. `ALIGN_NORM=1` turns it back on for
comparison only.

Each result also records `strictEq`, which is equality under normalisations 1, 2, 6 and 8 only.
`show.mjs --rebind` is a diagnostic that numbers each group definition separately, as if every id
were a unique symbol. It is useful for telling an id collision apart from other differences.

`selftest.mjs` checks both directions. A deep copy, and copies changed only in ways normalisations
3, 4, 5 and 8 cover, must compare equal. Seven kinds of single printer-visible mutation must compare
unequal: a changed string, a flipped group break, a changed line kind, swapped if-break branches, a
changed align width, indent turned into align, and dropped fill parts.

## Current result

After the second round of fixes:

- **Recorded corpus (16,935 cases):** 15,797 are doc-equal, with 0 latent and 0 string differences.
  The port declines 1,090. The 48 empty inputs give equal strings.
- **The first 200 cases of `../cases.jsonl`:** 159 are doc-equal, with 0 latent and 0 string differences, and 41
  declined.
- **The 23 cases from the second fuzz run:** 6 are doc-equal, with 0 latent and 0 string differences, and 17
  declined.
- **Width sweep (run after round 1):** every width-dependent corpus case the port formats is doc-equal
  at every width checked, and so is each of the 912 probes. Where Prettier's doc changes with width
  (61 corpus cases and 392 probes), the strings are equal at every width from 0 to 200.

The two mismatches this check found earlier are fixed, and are kept as cases in `../cases.jsonl`.
One was group-id collisions between embedded Markdown code blocks; the other was `--end-of-line cr`
in template-literal interpolations.
