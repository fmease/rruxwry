//! The command-line interface.

use crate::{
    build::{BuildOptions, CompileOptions, DebugOptions, DocOptions},
    data::{CrateNameBuf, CrateType, DocBackend, Edition, Identity},
    directive::Flavor,
    operate::{CompileMode, DocMode, Open, Operation, Run},
    utility::{Conjunction, ListingExt as _, parse},
};
use clap::ColorChoice;
use std::{ffi::OsString, path::PathBuf};

// FIXME: Improve naming: *Flags, Arguments, ...

pub(crate) fn arguments() -> Arguments {
    let mut args = std::env::args_os().peekable();

    let bin = args.next().into_iter();
    // FIXME: If this resembles a toolchain argument, throw an error suggesting to
    //        move it after the subcommand.
    let subcommand = args.next().into_iter();

    // FIXME: Ideally, `clap` would support custom prefixes (here: `+`).
    //        See <https://github.com/clap-rs/clap/issues/2468>.
    // FIXME: It would be nice if we could show this as `[+<TOOLCHAIN>]` or similar in the help output.
    // NOTE:  Currently we don't offer a way to manually specify the path to rust{c,doc}.
    let toolchain = args
        .peek()
        // FIXME: Is this actually correct on Windows (UTF-16 and all)?
        .filter(|arg| arg.as_encoded_bytes().starts_with(b"+"))
        .map(drop)
        .and_then(|()| args.next());

    let args = bin.chain(subcommand).chain(args);

    fn path() -> clap::Arg {
        clap::Arg::new(id::PATH)
            .required(true)
            .value_parser(clap::builder::ValueParser::path_buf())
            .help("Path to the source file")
    }
    fn verbatim() -> clap::Arg {
        clap::Arg::new(id::VERBATIM).num_args(..).last(true).value_name("VERBATIM")
    }
    fn compiletest() -> clap::Arg {
        clap::Arg::new(id::COMPILETEST)
            .short('@')
            .long("compiletest")
            // FIXME: Limit number of occurrences to 0..=2 (`max_occurences` no longer exists).
            .action(clap::ArgAction::Count)
            .help("Enable compiletest directives")
    }
    fn crate_name_and_type() -> impl Iterator<Item = clap::Arg> {
        [
            clap::Arg::new(id::CRATE_NAME)
                .short('n')
                .long("crate-name")
                .value_name("NAME")
                .value_parser(CrateNameBuf::parse_cli_style)
                .help("Set the name of the (base) crate"),
            clap::Arg::new(id::CRATE_TYPE)
                .short('t')
                .long("crate-type")
                .value_name("TYPE")
                .value_parser(CrateType::parse_cli_style)
                .help("Set the type of the (base) crate"),
        ]
        .into_iter()
    }
    fn edition() -> clap::Arg {
        clap::Arg::new(id::EDITION)
            .short('e')
            .long("edition")
            .value_parser(Edition::parse_cli_style)
            .help("Set the edition of the source files")
    }
    fn cfgs() -> impl Iterator<Item = clap::Arg> {
        [
            clap::Arg::new(id::CFGS)
                .long("cfg")
                .value_name("NAME")
                .action(clap::ArgAction::Append)
                .help("Enable a `cfg`"),
            clap::Arg::new(id::REVISION)
                .long("rev")
                .value_name("NAME")
                .requires(id::COMPILETEST)
                .help("Enable a compiletest revision"),
            clap::Arg::new(id::CARGO_FEATURES)
                .short('f')
                .long("cargo-feature")
                .value_name("NAME")
                .action(clap::ArgAction::Append)
                .help("Enable a Cargo-like feature"),
            // FIXME: This doesn't really belong in this "group" (`cfgs`)
            clap::Arg::new(id::RUSTC_FEATURES)
                .short('F')
                .long("rustc-feature")
                .value_name("NAME")
                .action(clap::ArgAction::Append)
                .help("Enable an experimental rustc library or language feature"),
        ]
        .into_iter()
    }
    fn extra() -> impl Iterator<Item = clap::Arg> {
        [
            clap::Arg::new(id::SUPPRESS_LINTS)
                .short('/')
                .long("suppress-lints")
                .action(clap::ArgAction::SetTrue)
                .help("Cap lints at allow level"),
            clap::Arg::new(id::INTERNALS)
                .short('#')
                .long("internals")
                .action(clap::ArgAction::SetTrue)
                .help("Enable rust{,do}c's `-Zverbose-internals`"),
            clap::Arg::new(id::NEXT_SOLVER)
                .short('N')
                .long("next-solver")
                .action(clap::ArgAction::SetTrue)
                .help("Enable the next-gen trait solver"),
            clap::Arg::new(id::IDENTITY)
                .short('I')
                .long("identity")
                .value_name("IDENTITY")
                .value_parser(Identity::parse_cli_style)
                .help("Force rust{,do}c's identity"),
            clap::Arg::new(id::LOG)
                .long("log")
                .value_name("FILTER")
                .require_equals(true)
                .num_args(..=1)
                .default_missing_value("debug")
                .help("Enable rust{,do}c logging. FILTER defaults to `debug`"),
            clap::Arg::new(id::NO_BACKTRACE)
                .short('B')
                .long("no-backtrace")
                .action(clap::ArgAction::SetTrue)
                .help("Override `RUST_BACKTRACE` to be `0`"),
            clap::Arg::new(id::VERBOSE)
                .short('V')
                .long("verbose")
                .action(clap::ArgAction::SetTrue)
                .help("Use verbose output"),
            clap::Arg::new(id::DRY_RUN)
                .short('0')
                .long("dry-run")
                .action(clap::ArgAction::SetTrue)
                // FIXME: Inaccurate description
                .help("Run through without making any changes"),
            clap::Arg::new(id::COLOR)
                .long("color")
                .value_name("WHEN")
                .default_value("auto")
                .value_parser(clap::builder::EnumValueParser::<ColorChoice>::new())
                .help("Control when to use color"),
        ]
        .into_iter()
    }

    // FIXME: Use `try_get_matches_from`. Blocker: Define an error type that leads to an exit code of 2 instead of 1.
    let mut matches = clap::Command::new(env!("CARGO_PKG_NAME"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .subcommand_required(true)
        .subcommands([
            clap::Command::new(id::BUILD)
                .alias("b")
                .about("Compile the given crate with rustc")
                .defer(|command| {
                    command
                        .arg(path())
                        .arg(verbatim().help("Flags passed to `rustc` verbatim"))
                        .arg(
                            clap::Arg::new(id::RUN)
                                .short('r')
                                .long("run")
                                .action(clap::ArgAction::SetTrue)
                                .help("Run the built binary"),
                        )
                        .arg(
                            clap::Arg::new(id::CHECK)
                                .short('c')
                                .long("check")
                                .action(clap::ArgAction::SetTrue)
                                .conflicts_with(id::RUN)
                                .help("Don't fully compile, only check the crate"),
                        )
                        .arg(compiletest())
                        .args(crate_name_and_type())
                        .arg(edition())
                        .args(cfgs())
                        .args(extra())
                }),
            clap::Command::new(id::DOC)
                .alias("d")
                .about("Document the given crate with rustdoc")
                .defer(|command| {
                    command
                        .arg(path())
                        .arg(verbatim().help("Flags passed to `rustc` and `rustdoc` verbatim"))
                        .arg(
                            clap::Arg::new(id::OPEN)
                                .short('o')
                                .long("open")
                                .action(clap::ArgAction::SetTrue)
                                .help("Open the generated docs in a browser"),
                        )
                        .arg(
                            clap::Arg::new(id::JSON)
                                .short('j')
                                .long("json")
                                .conflicts_with(id::OPEN)
                                .action(clap::ArgAction::SetTrue)
                                .help("Output JSON instead of HTML"),
                        )
                        .arg(compiletest())
                        .arg(
                            clap::Arg::new(id::CROSS_CRATE)
                                .short('X')
                                .long("cross-crate")
                                .action(clap::ArgAction::SetTrue)
                                .conflicts_with(id::COMPILETEST)
                                .help("Enable the cross-crate re-export mode"),
                        )
                        .args(crate_name_and_type())
                        .arg(
                            clap::Arg::new(id::CRATE_VERSION)
                                .short('v')
                                .long("crate-version")
                                .value_name("VERSION")
                                .help("Set the version of the (base) crate"),
                        )
                        .arg(edition())
                        .args(cfgs())
                        .args([
                            clap::Arg::new(id::PRIVATE)
                                .short('P')
                                .long("private")
                                .action(clap::ArgAction::SetTrue)
                                .help("Document private items"),
                            clap::Arg::new(id::HIDDEN)
                                .short('H')
                                .long("hidden")
                                .action(clap::ArgAction::SetTrue)
                                .help("Document hidden items"),
                            clap::Arg::new(id::LAYOUT)
                                .long("layout")
                                .action(clap::ArgAction::SetTrue)
                                .help("Document the memory layout of types"),
                            clap::Arg::new(id::LINK_TO_DEF)
                                .long("link-to-def")
                                .alias("ltd")
                                .action(clap::ArgAction::SetTrue)
                                .help("Generate links to definitions"),
                            clap::Arg::new(id::NORMALIZE)
                                .long("normalize")
                                .action(clap::ArgAction::SetTrue)
                                .help("Normalize types"),
                            clap::Arg::new(id::THEME)
                                .long("theme")
                                .default_value("ayu")
                                .help("Set the theme"),
                        ])
                        .args(extra())
                }),
        ])
        .get_matches_from(args);

    // unwrap: handled by `clap`.
    let (operation, mut matches) = matches.remove_subcommand().unwrap();

    let compiletest = match matches.remove_one::<u8>(id::COMPILETEST).unwrap_or_default() {
        0 => None,
        1 => Some(Flavor::Vanilla),
        // FIXME: Reject count > 2.
        _ => Some(Flavor::Rruxwry),
    };

    let operation = match operation.as_str() {
        id::BUILD => Operation::Compile {
            run: match matches.remove_one::<bool>(id::RUN).unwrap_or_default() {
                true => Run::Yes,
                false => Run::No,
            },
            mode: match compiletest {
                Some(flavor) => CompileMode::Compiletest(flavor),
                None => CompileMode::Default,
            },
            options: CompileOptions { check: matches.remove_one(id::CHECK).unwrap_or_default() },
        },
        id::DOC => Operation::Document {
            open: match matches.remove_one::<bool>(id::OPEN).unwrap_or_default() {
                true => Open::Yes,
                false => Open::No,
            },
            mode: match (matches.remove_one(id::CROSS_CRATE).unwrap_or_default(), compiletest) {
                (true, None) => DocMode::CrossCrate,
                (false, Some(flavor)) => DocMode::Compiletest(flavor),
                (false, None) => DocMode::Default,
                (true, Some(_)) => unreachable!(), // Already caught by `clap`.
            },
            options: DocOptions {
                backend: if matches.remove_one(id::JSON).unwrap_or_default() {
                    DocBackend::Json
                } else {
                    DocBackend::Html
                },
                crate_version: matches.remove_one(id::CRATE_VERSION),
                private: matches.remove_one(id::PRIVATE).unwrap_or_default(),
                hidden: matches.remove_one(id::HIDDEN).unwrap_or_default(),
                layout: matches.remove_one(id::LAYOUT).unwrap_or_default(),
                link_to_def: matches.remove_one(id::LINK_TO_DEF).unwrap_or_default(),
                normalize: matches.remove_one(id::NORMALIZE).unwrap_or_default(),
                theme: matches.remove_one(id::THEME).unwrap(),
            },
        },
        _ => unreachable!(), // handled by `clap`,
    };

    Arguments {
        toolchain,
        path: matches.remove_one(id::PATH).unwrap(),
        verbatim: matches.remove_many(id::VERBATIM).map(Iterator::collect).unwrap_or_default(),
        operation,
        crate_name: matches.remove_one(id::CRATE_NAME),
        crate_type: matches.remove_one(id::CRATE_TYPE),
        edition: matches.remove_one(id::EDITION),
        build: BuildOptions {
            cfgs: matches.remove_many(id::CFGS).map(Iterator::collect).unwrap_or_default(),
            revision: matches.remove_one(id::REVISION),
            cargo_features: matches
                .remove_many(id::CARGO_FEATURES)
                .map(Iterator::collect)
                .unwrap_or_default(),
            rustc_features: matches
                .remove_many(id::RUSTC_FEATURES)
                .map(Iterator::collect)
                .unwrap_or_default(),
            suppress_lints: matches.remove_one(id::SUPPRESS_LINTS).unwrap_or_default(),
            internals: matches.remove_one(id::INTERNALS).unwrap_or_default(),
            next_solver: matches.remove_one(id::NEXT_SOLVER).unwrap_or_default(),
            identity: matches.remove_one(id::IDENTITY),
            log: matches.remove_one(id::LOG),
            no_backtrace: matches.remove_one(id::NO_BACKTRACE).unwrap_or_default(),
        },
        debug: DebugOptions {
            verbose: matches.remove_one(id::VERBOSE).unwrap(),
            dry_run: matches.remove_one(id::DRY_RUN).unwrap(),
        },
        color: matches.remove_one(id::COLOR).unwrap(),
    }
}

pub(crate) struct Arguments {
    /// The toolchain, prefixed with `+`.
    pub(crate) toolchain: Option<OsString>,
    pub(crate) path: PathBuf,
    pub(crate) verbatim: Vec<String>,
    pub(crate) operation: Operation,
    pub(crate) crate_name: Option<CrateNameBuf>,
    pub(crate) crate_type: Option<CrateType>,
    pub(crate) edition: Option<Edition>,
    pub(crate) build: BuildOptions,
    pub(crate) debug: DebugOptions,
    pub(crate) color: ColorChoice,
}

impl Edition {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        parse!(
            "D" => Self::RUSTC_DEFAULT,
            "S" => Self::LATEST_STABLE,
            "E" => Self::BLEEDING_EDGE,
            "15" | "2015" => Self::Rust2015,
            "18" | "2018" => Self::Rust2018,
            "21" | "2021" => Self::Rust2021,
            "24" | "2024" => Self::Rust2024,
        )(source)
        .map_err(possible_values)
    }
}

impl CrateNameBuf {
    fn parse_cli_style(source: &str) -> Result<Self, &'static str> {
        Self::adjust_and_parse(source).map_err(|()| "not a non-empty alphanumeric string")
    }
}

impl CrateType {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        source.parse().map_err(possible_values)
    }
}

impl Identity {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        parse!(
            "T" => Self::True,
            "S" => Self::Stable,
            "N" => Self::Nightly,
        )(source)
        .map_err(possible_values)
    }
}

fn possible_values(values: impl Iterator<Item: std::fmt::Display> + Clone) -> String {
    format!(
        "possible values: {}",
        values.into_iter().map(|value| format!("`{value}`")).list(Conjunction::Or)
    )
}

mod id {
    pub(super) const BUILD: &str = "build";
    pub(super) const CARGO_FEATURES: &str = "CARGO_FEATURES";
    pub(super) const CFGS: &str = "CFGS";
    pub(super) const CHECK: &str = "CHECK";
    pub(super) const COLOR: &str = "COLOR";
    pub(super) const COMPILETEST: &str = "COMPILETEST";
    pub(super) const CRATE_NAME: &str = "CRATE_NAME";
    pub(super) const CRATE_TYPE: &str = "CRATE_TYPE";
    pub(super) const CRATE_VERSION: &str = "CRATE_VERSION";
    pub(super) const CROSS_CRATE: &str = "CROSS_CRATE";
    pub(super) const DOC: &str = "doc";
    pub(super) const DRY_RUN: &str = "DRY_RUN";
    pub(super) const EDITION: &str = "EDITION";
    pub(super) const HIDDEN: &str = "HIDDEN";
    pub(super) const IDENTITY: &str = "IDENTITY";
    pub(super) const INTERNALS: &str = "INTERNALS";
    pub(super) const JSON: &str = "JSON";
    pub(super) const LAYOUT: &str = "LAYOUT";
    pub(super) const LINK_TO_DEF: &str = "LINK_TO_DEF";
    pub(super) const LOG: &str = "LOG";
    pub(super) const NEXT_SOLVER: &str = "NEXT_SOLVER";
    pub(super) const NO_BACKTRACE: &str = "NO_BACKTRACE";
    pub(super) const NORMALIZE: &str = "NORMALIZE";
    pub(super) const OPEN: &str = "OPEN";
    pub(super) const PATH: &str = "PATH";
    pub(super) const PRIVATE: &str = "PRIVATE";
    pub(super) const REVISION: &str = "REVISION";
    pub(super) const RUN: &str = "RUN";
    pub(super) const RUSTC_FEATURES: &str = "RUSTC_FEATURES";
    pub(super) const SUPPRESS_LINTS: &str = "SUPPRESS_LINTS";
    pub(super) const THEME: &str = "THEME";
    pub(super) const VERBATIM: &str = "VERBATIM";
    pub(super) const VERBOSE: &str = "VERBOSE";
}
