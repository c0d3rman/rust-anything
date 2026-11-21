# Decisions: ESLint to Rust

Original: ESLint 10.15.0 (tag `v10.15.0`, commit a2101d992ab8a5d49ab7c960e9f5f67707530703), its
production dependencies installed with `npm install --omit=dev`, copied to
`node_modules/eslint` of the app folder.

## ADR-001: Scope is the CLI's stdin path with flag-only config
Status: accepted (2026-11-20)
Context: ESLint's config is a JavaScript file (`eslint.config.js`) that has to be evaluated, and
plugins, parsers and formatters are JavaScript modules loaded at run time. A Rust front door
cannot evaluate them. `--no-config-lookup` with `--rule`, `--global` and `--parser-options` gives
a complete config without a file, and `--stdin` gives a file without file enumeration, ignore
files or caching.
Decision: the front door lints in Rust when the run is `--no-config-lookup ... --stdin` with
only these flags: `--rule`, `--global`, `--parser-options` (`ecmaFeatures.jsx` and
`globalReturn`), `-f json|stylish`, `--fix-dry-run`, `--fix-type`, `--stdin-filename` (`.js`,
`.mjs`, `.cjs` inside the working directory), `--quiet`, `--max-warnings`,
`--report-unused-disable-directives(-severity)`, `--report-unused-inline-configs`,
`--no-inline-config`, `--color`/`--no-color`, `--exit-on-fatal-error`. Everything else goes to
ESLint unchanged.
Consequences: config files, plugins, custom parsers (TypeScript, Babel), processors, other
formatters, file arguments, `--fix`, caching and suppressions stay in Node. The port is measured
on what it claims; nothing is claimed without a recorded case.

## ADR-002: Rule set
Status: accepted (2026-11-20)
Decision: the 64 rules of `eslint:recommended` plus 26 widely used ones (`scripts/rules.txt`).
A run that enables a rule not ported, or a rule option the port does not validate, goes to
ESLint.

## ADR-003: oxc for parsing, ESLint's code for everything after
Status: accepted (2026-11-20)
Context: espree (acorn) gives the AST, tokens and comments every rule reads. oxc's ESTree output,
with its token list mapped to espree's token types and ranges converted to UTF-16 code units,
matches espree's nodes, ranges, tokens and comments on 30,082 of 30,099 inputs taken from all
of ESLint's rule tests (checked by `linter-rs/examples/dump.rs` against espree); the other 17
are refused.
Decision: parse with `oxc_parser` 0.158; port ESLint's own code for the rest (SourceCode, token
store, directives, report translator, fixer, eslint-scope, code path analysis, the rules), not
oxc's semantic analysis or Oxlint's rules, which differ from ESLint.
Consequences: any file oxc rejects, or that oxc's semantic syntax checks reject (early errors
acorn reports while parsing), goes to ESLint, so parse errors keep espree's messages.
Import assertions (`assert { type: "json" }`), which oxc accepts and acorn rejects, are treated
as a parse error and go to ESLint too.

## ADR-004: Corpus
Status: accepted (2026-11-20)
Decision: ESLint's own RuleTester cases for the 90 rules, as CLI runs (`scripts/harvest-rule-tests.cjs`:
options as `--rule`, globals as `--global`, `commonjs` and `script` source types as `.cjs`,
every fifth case through `stylish`, invalid cases with an `output` through `--fix-dry-run`);
cases with a custom parser (2,094), cases that use a test-only plugin's rule (10) and one with
a lone surrogate (which stdin cannot carry) are left out; 34 runs that came out identical after
the translation were recorded once. Real files: ESLint's own `lib/` and `bin/`, ms
`3.0.0-canary.1`, Fuse.js `v7.1.0`, json-server `v1.0.0-beta.3`, each linted with all 90 rules
through `json` with `--fix-dry-run` and through `stylish` (`scripts/gen-real-inputs.cjs`).
Linter core cases: directives, inline config, unused directives, fixes, formats, CLI options
(`scripts/gen-core-inputs.cjs`). Plus the tool's traps.

## ADR-005: Equivalence bar
Status: accepted (2026-11-20)
Decision: byte-identical stdout, stderr and exit code, with the working directory written as
`[CWD]` by the replay. Parse errors, config errors and crashes stay in ESLint.

## ADR-006: Gate unit
Status: accepted (2026-11-20)
Context: ESLint is CommonJS and its linter is a class; the tool's bridge reads exported
functions.
Decision: `wrappers/eslint-stdin.js` exports `lintStdin(args, text)`, which runs ESLint's own
`cli.execute` in process and returns `{ stdout, stderr, code }` with the working directory as
`[CWD]`. It is bridged as `eslint-stdin-rs`, recorded on the CLI cases the port claims, and
backed by `linter-rs`.
