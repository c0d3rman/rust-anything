//! The options the JS printer reads, and the CLI flags that set them.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailingComma {
    All,
    Es5,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteProps {
    AsNeeded,
    Consistent,
    Preserve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parser {
    Babel,
    Typescript,
    Json,
    Jsonc,
    Json5,
    JsonStringify,
    Graphql,
    Yaml,
    Markdown,
    Css,
    Scss,
    Less,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProseWrap {
    Always,
    Never,
    Preserve,
}

const YAML_EXTENSIONS: [&str; 10] = [
    ".yml",
    ".mir",
    ".reek",
    ".rviz",
    ".sublime-syntax",
    ".syntax",
    ".yaml",
    ".yaml-tmlanguage",
    ".yaml.sed",
    ".yml.mysql",
];
const MARKDOWN_EXTENSIONS: [&str; 11] = [
    ".md",
    ".livemd",
    ".markdown",
    ".mdown",
    ".mdwn",
    ".mkd",
    ".mkdn",
    ".mkdown",
    ".ronn",
    ".scd",
    ".workbook",
];
const YAML_FILES: [&str; 10] = [
    ".clang-format",
    ".clang-tidy",
    ".clangd",
    ".gemrc",
    "CITATION.cff",
    "glide.lock",
    "pixi.lock",
    ".prettierrc",
    ".stylelintrc",
    ".lintstagedrc",
];

impl Parser {
    pub fn is_json(self) -> bool {
        matches!(
            self,
            Parser::Json | Parser::Jsonc | Parser::Json5 | Parser::JsonStringify
        )
    }
}

const JSON_STRINGIFY_FILES: [&str; 3] = ["package.json", "package-lock.json", "composer.json"];
const JSON_FILES: [&str; 15] = [
    ".all-contributorsrc",
    ".arcconfig",
    ".auto-changelog",
    ".c8rc",
    ".htmlhintrc",
    ".imgbotconfig",
    ".nycrc",
    ".tern-config",
    ".tern-project",
    ".watchmanconfig",
    ".babelrc",
    ".jscsrc",
    ".jshintrc",
    ".jslintrc",
    ".swcrc",
];
const JSON_EXTENSIONS: [&str; 17] = [
    ".json",
    ".avsc",
    ".geojson",
    ".gltf",
    ".har",
    ".ice",
    ".json.example",
    ".mcmeta",
    ".sarif",
    ".tact",
    ".tfstate",
    ".tfstate.backup",
    ".topojson",
    ".webapp",
    ".webmanifest",
    ".yy",
    ".yyp",
];
const JSONC_EXTENSIONS: [&str; 17] = [
    ".jsonc",
    ".code-snippets",
    ".code-workspace",
    ".sublime-build",
    ".sublime-color-scheme",
    ".sublime-commands",
    ".sublime-completions",
    ".sublime-keymap",
    ".sublime-macro",
    ".sublime-menu",
    ".sublime-mousemap",
    ".sublime-project",
    ".sublime-settings",
    ".sublime-theme",
    ".sublime-workspace",
    ".sublime_metrics",
    ".sublime_session",
];

fn json_parser_for(filepath: &str) -> Option<Parser> {
    let base = filepath
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(filepath)
        .to_lowercase();
    if JSON_STRINGIFY_FILES.contains(&base.as_str()) {
        return Some(Parser::JsonStringify);
    }
    if JSON_FILES.contains(&base.as_str()) {
        return Some(Parser::Json);
    }
    if base.ends_with(".importmap") {
        return Some(Parser::JsonStringify);
    }
    if JSON_EXTENSIONS.iter().any(|e| base.ends_with(e)) {
        return Some(Parser::Json);
    }
    if JSONC_EXTENSIONS.iter().any(|e| base.ends_with(e)) {
        return Some(Parser::Jsonc);
    }
    if base.ends_with(".json5") {
        return Some(Parser::Json5);
    }
    None
}

#[derive(Clone, Debug)]
pub struct Options {
    pub print_width: usize,
    pub tab_width: usize,
    pub use_tabs: bool,
    pub semi: bool,
    pub single_quote: bool,
    pub jsx_single_quote: bool,
    pub quote_props: QuoteProps,
    pub trailing_comma: TrailingComma,
    pub bracket_spacing: bool,
    pub bracket_same_line: bool,
    pub object_wrap_preserve: bool,
    pub arrow_parens_always: bool,
    pub experimental_ternaries: bool,
    pub operator_position_start: bool,
    pub parser: Parser,
    pub filepath: String,
    pub end_of_line: EndOfLine,
    pub prose_wrap: ProseWrap,
    pub parent_markdown: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EndOfLine {
    Lf,
    Crlf,
    Cr,
    Auto,
}

impl EndOfLine {
    /// `convertEndOfLine`; `format` resolves `Auto` before printing.
    pub fn as_str(self) -> &'static str {
        match self {
            EndOfLine::Crlf => "\r\n",
            EndOfLine::Cr => "\r",
            EndOfLine::Lf | EndOfLine::Auto => "\n",
        }
    }
}

impl Options {
    pub fn for_path(filepath: &str) -> Option<Self> {
        Self::for_path_with(filepath, None)
    }

    pub fn for_path_with(filepath: &str, forced: Option<Parser>) -> Option<Self> {
        let lower = filepath.to_ascii_lowercase();
        let parser = if let Some(p) = forced {
            p
        } else if [".graphql", ".gql", ".graphqls"]
            .iter()
            .any(|e| lower.ends_with(e))
        {
            Parser::Graphql
        } else if [".js", ".jsx", ".mjs", ".cjs"]
            .iter()
            .any(|e| lower.ends_with(e))
        {
            Parser::Babel
        } else if [".ts", ".tsx", ".mts", ".cts"]
            .iter()
            .any(|e| lower.ends_with(e))
        {
            Parser::Typescript
        } else if let Some(p) = json_parser_for(filepath) {
            p
        } else {
            let base = filepath.rsplit(['/', '\\']).next().unwrap_or(filepath);
            let lower_base = base.to_lowercase();
            if lower_base == "contents.lr" || lower_base == "readme" {
                Parser::Markdown
            } else if YAML_FILES.contains(&base)
                || YAML_EXTENSIONS.iter().any(|e| lower_base.ends_with(e))
            {
                Parser::Yaml
            } else if MARKDOWN_EXTENSIONS.iter().any(|e| lower_base.ends_with(e)) {
                Parser::Markdown
            } else if [".css", ".wxss", ".pcss", ".postcss"]
                .iter()
                .any(|e| lower_base.ends_with(e))
            {
                Parser::Css
            } else if lower_base.ends_with(".scss") {
                Parser::Scss
            } else if lower_base.ends_with(".less") {
                Parser::Less
            } else {
                return None;
            }
        };
        Some(Options {
            print_width: 80,
            tab_width: 2,
            use_tabs: false,
            semi: true,
            single_quote: false,
            jsx_single_quote: false,
            quote_props: QuoteProps::AsNeeded,
            trailing_comma: TrailingComma::All,
            bracket_spacing: true,
            bracket_same_line: false,
            object_wrap_preserve: true,
            arrow_parens_always: true,
            experimental_ternaries: false,
            operator_position_start: false,
            parser,
            filepath: filepath.to_string(),
            end_of_line: EndOfLine::Lf,
            prose_wrap: ProseWrap::Preserve,
            parent_markdown: false,
        })
    }

    /// Reads the Prettier CLI arguments this port understands: `--stdin-filepath <path>` and the
    /// formatting flags. Anything else returns `None` and the original handles the call.
    pub fn from_cli(args: &[String]) -> Option<Self> {
        let mut path = None;
        let mut flags: Vec<(String, Option<String>)> = Vec::new();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
                _ => (arg.clone(), None),
            };
            let takes_value = matches!(
                name.as_str(),
                "--stdin-filepath"
                    | "--print-width"
                    | "--tab-width"
                    | "--trailing-comma"
                    | "--arrow-parens"
                    | "--quote-props"
                    | "--object-wrap"
                    | "--experimental-operator-position"
                    | "--end-of-line"
                    | "--parser"
                    | "--prose-wrap"
            );
            let value = if takes_value {
                match inline {
                    Some(v) => Some(v),
                    None => {
                        i += 1;
                        Some(args.get(i)?.clone())
                    }
                }
            } else {
                if inline.is_some() {
                    return None;
                }
                None
            };
            if name == "--stdin-filepath" {
                path = value;
            } else {
                flags.push((name, value));
            }
            i += 1;
        }
        let mut forced = None;
        for (name, value) in &flags {
            if name == "--parser" {
                forced = match value.as_deref() {
                    Some("graphql") => Some(Parser::Graphql),
                    Some("yaml") => Some(Parser::Yaml),
                    Some("markdown") => Some(Parser::Markdown),
                    Some("css") => Some(Parser::Css),
                    Some("scss") => Some(Parser::Scss),
                    Some("less") => Some(Parser::Less),
                    _ => return None,
                };
            }
        }
        let mut o = Options::for_path_with(&path?, forced)?;
        for (name, value) in flags {
            let v = value.as_deref().unwrap_or("");
            match name.as_str() {
                "--parser" => {}
                "--prose-wrap" => {
                    o.prose_wrap = match v {
                        "always" => ProseWrap::Always,
                        "never" => ProseWrap::Never,
                        "preserve" => ProseWrap::Preserve,
                        _ => return None,
                    }
                }
                "--print-width" => o.print_width = parse_int(v)?,
                "--tab-width" => o.tab_width = parse_int(v)?,
                "--use-tabs" => o.use_tabs = true,
                "--no-semi" => o.semi = false,
                "--single-quote" => o.single_quote = true,
                "--jsx-single-quote" => o.jsx_single_quote = true,
                "--no-bracket-spacing" => o.bracket_spacing = false,
                "--bracket-same-line" => o.bracket_same_line = true,
                "--experimental-ternaries" => o.experimental_ternaries = true,
                "--trailing-comma" => {
                    o.trailing_comma = match v {
                        "all" => TrailingComma::All,
                        "es5" => TrailingComma::Es5,
                        "none" => TrailingComma::None,
                        _ => return None,
                    }
                }
                "--arrow-parens" => {
                    o.arrow_parens_always = match v {
                        "always" => true,
                        "avoid" => false,
                        _ => return None,
                    }
                }
                "--quote-props" => {
                    o.quote_props = match v {
                        "as-needed" => QuoteProps::AsNeeded,
                        "consistent" => QuoteProps::Consistent,
                        "preserve" => QuoteProps::Preserve,
                        _ => return None,
                    }
                }
                "--object-wrap" => {
                    o.object_wrap_preserve = match v {
                        "preserve" => true,
                        "collapse" => false,
                        _ => return None,
                    }
                }
                "--end-of-line" => {
                    o.end_of_line = match v {
                        "lf" => EndOfLine::Lf,
                        "crlf" => EndOfLine::Crlf,
                        "cr" => EndOfLine::Cr,
                        "auto" => EndOfLine::Auto,
                        _ => return None,
                    }
                }
                "--experimental-operator-position" => {
                    o.operator_position_start = match v {
                        "start" => true,
                        "end" => false,
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        Some(o)
    }

    pub fn should_print_comma(&self, es5: bool) -> bool {
        match self.trailing_comma {
            TrailingComma::All => true,
            TrailingComma::Es5 => es5,
            TrailingComma::None => false,
        }
    }
}

fn parse_int(v: &str) -> Option<usize> {
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse().ok()
}
