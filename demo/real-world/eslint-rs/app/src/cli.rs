use std::ffi::OsString;

use clap::{Arg, ArgAction, Command, value_parser};

/// The command `units::dispatch` gets when argv names no subcommand.
pub const ROOT: &str = "eslint";

fn rest() -> Arg {
    Arg::new("args")
        .num_args(0..)
        .trailing_var_arg(true)
        .allow_hyphen_values(true)
        .value_parser(value_parser!(OsString))
}

/// Routes argv to a command. Anything this parser rejects runs the original, so it never
/// has to match every way the original accepts its arguments.
pub fn command() -> Command {
    Command::new(ROOT)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(Arg::new("color").long("color").action(ArgAction::Count))
        .arg(
            Arg::new("exit-on-fatal-error")
                .long("exit-on-fatal-error")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("fix-dry-run")
                .long("fix-dry-run")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("fix-type")
                .long("fix-type")
                .action(ArgAction::Count),
        )
        .arg(Arg::new("global").long("global").action(ArgAction::Count))
        .arg(Arg::new("help").long("help").action(ArgAction::Count))
        .arg(
            Arg::new("max-warnings")
                .long("max-warnings")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("no-color")
                .long("no-color")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("no-config-lookup")
                .long("no-config-lookup")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("no-inline-config")
                .long("no-inline-config")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("parser-options")
                .long("parser-options")
                .action(ArgAction::Count),
        )
        .arg(Arg::new("quiet").long("quiet").action(ArgAction::Count))
        .arg(
            Arg::new("report-unused-disable-directives")
                .long("report-unused-disable-directives")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("report-unused-disable-directives-severity")
                .long("report-unused-disable-directives-severity")
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("report-unused-inline-configs")
                .long("report-unused-inline-configs")
                .action(ArgAction::Count),
        )
        .arg(Arg::new("rule").long("rule").action(ArgAction::Count))
        .arg(Arg::new("short-f").short('f').action(ArgAction::Count))
        .arg(Arg::new("stdin").long("stdin").action(ArgAction::Count))
        .arg(
            Arg::new("stdin-filename")
                .long("stdin-filename")
                .action(ArgAction::Count),
        )
        .arg(Arg::new("version").long("version").action(ArgAction::Count))
        .arg(rest())
}
