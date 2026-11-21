# Map: Prettier to Rust

Goal: Prettier's CLI as a Rust binary, JS/TS first. Decisions: [DECISIONS.md](DECISIONS.md).

| Unit | State | Result |
| --- | --- | --- |
| front door (`rust/app`) | done | 17224/17224; fallback 7.9% with js-format wired |
| CLI config resolution (`.prettierrc` JSON, `package.json`, overrides, `.editorconfig`, ignore files) | done | in the glue (`js_format/config.rs`); 46 of 63 config cases in Rust |
| doc printer (`printDocToString`, `getStringWidth`) | done | gate passes, 2848/2848 cases; crate `doc-printer-rs` |
| string/number/width utilities | done | inside the doc printer and `prettier-js-rs` |
| JS printer: statements, expressions, member chains, calls, assignments, binaries, arrays, objects, functions, arrows, modules, templates, ternaries, classes, decorators, JSX, TypeScript, prettier-ignore, parens (`js-format-rs` + `prettier-js-rs`) | wired | gate passes, 15753/15753 API cases; 15837/17224 CLI cases in Rust |
| JSON (json, jsonc, json5, json-stringify) | done | 995 of 1019 JSON cases Prettier formats run in Rust |
| GraphQL (graphql-js lexer/parser, generic comments, `printer-graphql.js`) | done | all 787 GraphQL cases Prettier formats run in Rust (`graphql.rs`) |
| YAML (yaml 1.10 CST parser and document checks, yaml-unist-parser transform and comment attachment, `printer-yaml.js`) | done | 3309 of 3310 YAML cases Prettier formats run in Rust (`src/yaml/`) |
| Markdown (remark-parse with footnotes, math, front matter, liquid and wiki links; `printer-markdown.js`; code blocks in JS, TS, JSON, GraphQL, YAML, Markdown, CSS, SCSS and Less formatted by the ports) | done | 6305 of 6350 Markdown cases in Rust; the rest have HTML code blocks, or embedded code oxc does not parse (`src/markdown/`) |
| CSS, SCSS, Less (postcss with postcss-scss and postcss-less, values, selector and media query parsers, `printer-postcss.js`) | done | all 2004 CSS/SCSS/Less cases Prettier formats run in Rust (`src/css/`) |
| comments (attach + JS handlers + printing) | done | inside `prettier-js-rs` (`comments.rs`) |

Out of scope for milestone 1: CSS, HTML, plugins, flow-only syntax (CSS, SCSS and Less were ported later).

Not ported yet (fall back): YAML/TOML/JSON5/JavaScript config files, embedded languages (CSS, GraphQL, HTML, Markdown in templates), Babel proposal syntax oxc does not parse, CLI flags beyond the common options; errors stay on Prettier.
