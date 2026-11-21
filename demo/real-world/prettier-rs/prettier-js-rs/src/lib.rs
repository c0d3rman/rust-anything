//! A Rust port of Prettier's JavaScript/TypeScript printer, for the subset of the language listed
//! in `estree.rs`. Files this port cannot print return [`Unsupported`], and the caller hands them
//! to Prettier. Prettier is MIT licensed, Copyright (c) James Long and contributors.
//! `src/es5-identifier.txt` is the pattern of `is-es5-identifier-name`, MIT licensed,
//! Copyright (c) fisker Cheung.

pub mod array;
pub mod arrow;
pub mod assignment;
pub mod binaryish;
pub mod call;
pub mod class;
pub mod comments;
pub mod css;
pub mod doc;
pub mod estree;
pub mod function;
pub mod graphql;
mod id_chars;
pub mod json;
pub mod jsx;
pub mod markdown;
pub mod member;
pub mod module;
pub mod node;
pub mod object;
pub mod options;
pub mod parens;
mod postprocess;
pub mod printer;
pub mod property;
pub mod soundness;
pub mod template;
pub mod ternary;
pub mod text;
pub mod typescript;
pub mod utils;
pub mod value;
pub mod yaml;

pub use options::Options;
pub use printer::Unsupported;

use printer::{Printer, R, bail};

const ACCEPTED_DIAGNOSTICS: [&str; 16] = [
    "Initializers are not allowed in ambient contexts.",
    "'private' modifier cannot be used with 'abstract' modifier.",
    "Type argument list cannot be empty.",
    "A 'return' statement can only be used within a function body.",
    "'const' modifier can only appear on a type parameter of a function, method or class",
    "'public' modifier must precede 'readonly' modifier.",
    "'readonly' modifier already seen.",
    "A 'declare' modifier cannot be used in an already ambient context.",
    "A parameter property may not be declared using a binding pattern.",
    "A required element cannot follow an optional element.",
    "A required parameter cannot follow an optional parameter.",
    "A type-only import can specify a default import or named bindings, but not both.",
    "Classes can't have a field named 'constructor'",
    "Only ambient modules can use quoted names.",
    "Unexpected new.target expression",
    AWAIT_DIAGNOSTIC,
];

const AWAIT_DIAGNOSTIC: &str =
    "`await` is only allowed within async functions and at the top levels of modules";

/// Formats `text` as Prettier does with `opts`, or says why this port cannot.
pub fn format(text: &str, opts: &Options) -> R<String> {
    let (has_bom, text) = match text.strip_prefix('\u{FEFF}') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    // Prettier resolves `auto` (`guessEndOfLine`) before printing, and printers that render docs
    // to strings (template literal interpolations) print with the resolved line ending.
    let end_of_line = match opts.end_of_line {
        options::EndOfLine::Auto => match text.find('\r') {
            Some(i) if text.as_bytes().get(i + 1) == Some(&b'\n') => options::EndOfLine::Crlf,
            Some(_) => options::EndOfLine::Cr,
            None => options::EndOfLine::Lf,
        },
        other => other,
    };
    let resolved;
    let opts = if opts.end_of_line == end_of_line {
        opts
    } else {
        resolved = Options {
            end_of_line,
            ..opts.clone()
        };
        &resolved
    };
    let normalized;
    let text = if text.contains('\r') {
        normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        normalized.as_str()
    } else {
        text
    };
    let formatted = format_text(text, opts, end_of_line.as_str())?;
    Ok(if has_bom {
        format!("\u{FEFF}{formatted}")
    } else {
        formatted
    })
}

fn format_text(text: &str, opts: &Options, eol: &'static str) -> R<String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let doc = text_to_doc(text, opts)?;
    print_doc(&doc, opts, eol)
}

/// The doc Prettier prints for `text` with `opts.parser`, before `printDocToString`.
pub fn text_to_doc(text: &str, opts: &Options) -> R<doc_printer_rs::port::Doc> {
    if matches!(
        opts.parser,
        options::Parser::Css | options::Parser::Scss | options::Parser::Less
    ) {
        return css::print(text, opts);
    }
    if opts.parser == options::Parser::Markdown {
        return markdown::print(text, opts);
    }
    if opts.parser == options::Parser::Yaml {
        return yaml::print(text, opts);
    }
    if opts.parser == options::Parser::Graphql {
        return graphql::print(text, opts);
    }
    if opts.parser.is_json() {
        let mut json_opts = opts.clone();
        if opts.parser == options::Parser::Json {
            json_opts.trailing_comma = options::TrailingComma::None;
        }
        let (ast, comment_list) = json::parse(text, opts.parser)?;
        let doc = if opts.parser == options::Parser::JsonStringify {
            json::print_stringify(&ast)?
        } else {
            let attached = match comments::attach(&ast, comment_list, text, false, false) {
                Ok(c) => c,
                Err(e) => return bail(format!("comments: {e}")),
            };
            let mut printer = Printer::new(text, &json_opts, &ast, attached);
            let doc = printer.print_current(printer::Args::default())?;
            if !printer.comments.all_printed() {
                return bail("comment not printed");
            }
            doc
        };
        return Ok(doc);
    }
    let lower = opts.filepath.to_ascii_lowercase();
    let mut source_type = oxc_span::SourceType::from_path(&opts.filepath)
        .unwrap_or_else(|_| oxc_span::SourceType::mjs());
    if lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs") {
        source_type = source_type.with_jsx(true);
    }
    source_type = source_type.with_module(!(lower.ends_with(".cjs") || lower.ends_with(".cts")));
    let alloc = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&alloc, text, source_type)
        .with_options(oxc_parser::ParseOptions {
            preserve_parens: true,
            ..Default::default()
        })
        .parse();
    if ret
        .diagnostics
        .errors()
        .chain(ret.diagnostics.warnings())
        .any(|d| {
            let d = d.to_string();
            !ACCEPTED_DIAGNOSTICS.contains(&d.as_str())
                // Babel rejects `await` outside async functions and module top levels.
                || (d == AWAIT_DIAGNOSTIC && !source_type.is_typescript())
        })
    {
        return bail("parse error");
    }
    let comment_list = comments::from_oxc(&ret.program, text);
    let json = ret
        .program
        .to_estree_json(source_type.is_typescript(), false);
    let mut ast: crate::value::Value = match crate::value::from_estree_json(&json) {
        Ok(v) => v,
        Err(e) => return bail(format!("estree json: {e}")),
    };
    soundness::check(
        &ast,
        &soundness::Source {
            text,
            comments: &comment_list,
            ts: source_type.is_typescript(),
            tsx: source_type.is_typescript() && source_type.is_jsx(),
        },
    )?;
    postprocess::postprocess(&mut ast, text, &comment_list, source_type.is_typescript())?;
    let attached = match comments::attach(
        &ast,
        comment_list,
        text,
        opts.experimental_ternaries,
        !source_type.is_typescript(),
    ) {
        Ok(c) => c,
        Err(e) => return bail(format!("comments: {e}")),
    };
    let mut printer = Printer::new(text, opts, &ast, attached);
    let doc = printer.print_current(printer::Args::default())?;
    if !printer.comments.all_printed() {
        return bail("comment not printed");
    }
    Ok(doc)
}

fn print_doc(doc: &doc_printer_rs::port::Doc, opts: &Options, eol: &'static str) -> R<String> {
    let print_opts = doc_printer_rs::port::PrintOptions {
        print_width: opts.print_width as f64,
        tab_width: opts.tab_width as f64,
        use_tabs: opts.use_tabs,
        end_of_line: eol,
    };
    if let Err(e) = doc_printer_rs::port::propagate_breaks(doc) {
        return bail(format!("doc: {e}"));
    }
    match doc_printer_rs::port::print(doc, &print_opts) {
        Ok(printed) => Ok(printed.formatted),
        Err(e) => bail(format!("doc: {e}")),
    }
}
