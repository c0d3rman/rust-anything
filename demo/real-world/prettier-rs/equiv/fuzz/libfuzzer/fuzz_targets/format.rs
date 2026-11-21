//! Coverage-guided growth of a corpus for prettier-js-rs. Panics are caught (and recorded by the
//! differential driver later) so that growth continues past them.
#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../header.rs"]
mod header;

static QUIET: std::sync::Once = std::sync::Once::new();

fuzz_target!(|data: &[u8]| {
    QUIET.call_once(|| std::panic::set_hook(Box::new(|_| {})));
    let Some((args, text)) = header::decode(data) else {
        return;
    };
    let Some(opts) = prettier_js_rs::Options::from_cli(&args) else {
        return;
    };
    let _ = std::panic::catch_unwind(|| {
        if let Ok(out) = prettier_js_rs::format(text, &opts) {
            let _ = prettier_js_rs::format(&out, &opts);
        }
    });
});
