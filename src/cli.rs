//! The command-line interface.

use crate::{
    builder::BuildMode,
    data::{CrateNameBuf, CrateType, Edition},
};
use clap::ColorChoice;
use joinery::JoinableIterator;
use std::{ffi::OsString, path::PathBuf};

// FIXME: Improve naming: *Flags, Arguments, ...
// FIXME: Subcommands: build, doc

pub(crate) fn arguments() -> Arguments {
    let mut args = std::env::args_os().peekable();

    let bin = args.next();

    // FIXME: Ideally, clap would support custom prefixes (here `+` over e.g. `-` or `--`).
    //        See <https://github.com/clap-rs/clap/issues/2468>.
    // FIXME: Consider only looking for the toolchain if the env var `RUSTUP_HOME` exists.
    // NOTE:  Currently we don't offer a way to manually specify the path to rust{c,doc}.
    let toolchain = args
        .peek()
        // FIXME: Is this actually correct on Windows? Wouldn't it be `\0+` (or is it '+\0')?
        .filter(|arg| arg.as_encoded_bytes().starts_with(b"+"))
        .map(drop)
        .and_then(|_| args.next());

    let args = bin.into_iter().chain(args);

    // FIXME: Use `try_get_matches_from`. Blocker: Define an error type that leads to an exit code of 2 instead of 1.
    let mut matches = clap::Command::new(env!("CARGO_PKG_NAME"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .args([
            clap::Arg::new(id::PATH)
                .required(true)
                .value_parser(clap::builder::ValueParser::path_buf())
                .help("Path to the source file"),
            // FIXME: .num_args(1..) .value_parser(String)
            clap::Arg::new(id::VERBATIM)
                .num_args(..)
                .last(true)
                .value_name("VERBATIM")
                .help("Flags passed to `rustc` and `rustdoc` verbatim"),
            clap::Arg::new(id::OPEN)
                .short('o')
                .long("open")
                .action(clap::ArgAction::SetTrue)
                .help("Open the generated docs in a browser"),
            clap::Arg::new(id::CRATE_NAME)
                .short('n')
                .long("crate-name")
                .value_name("NAME")
                .value_parser(CrateNameBuf::parse_cli_style)
                .help("Set the name of the (base) crate"),
            clap::Arg::new(id::CRATE_TYPE)
                .short('y')
                .long("crate-type")
                .value_name("TYPE")
                .value_parser(CrateType::parse_cli_style)
                .help("Set the type of the (base) crate"),
            clap::Arg::new(id::EDITION)
                .short('e')
                .long("edition")
                .value_parser(Edition::parse_cli_style)
                .help("Set the edition of the source files"),
            clap::Arg::new(id::CFGS)
                .long("cfg")
                .value_name("NAME")
                .action(clap::ArgAction::Append)
                .help("Enable a `cfg`"),
            clap::Arg::new(id::REVISIONS)
                .long("rev")
                .value_name("NAME")
                .action(clap::ArgAction::Append)
                .help("Enable a compiletest revision"),
            clap::Arg::new(id::CARGO_FEATURES)
                .short('f')
                .long("cargo-feature")
                .value_name("NAME")
                .action(clap::ArgAction::Append)
                .help("Enable a Cargo-like feature"),
            clap::Arg::new(id::RUSTC_FEATURES)
                .short('F')
                .long("rustc-feature")
                .value_name("NAME")
                .action(clap::ArgAction::Append)
                .help("Enable an experimental rustc library or language feature"),
            clap::Arg::new(id::JSON)
                .short('j')
                .long("json")
                .conflicts_with(id::OPEN)
                .action(clap::ArgAction::SetTrue)
                .help("Output JSON instead of HTML"),
            clap::Arg::new(id::CRATE_VERSION)
                .short('V')
                .long("crate-version")
                .value_name("VERSION")
                .help("Set the version of the (base) crate"),
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
            clap::Arg::new(id::LINK_TO_DEFINITION)
                .long("link-to-definition")
                .alias("ltd")
                .action(clap::ArgAction::SetTrue)
                .help("Generate links to definitions"),
            clap::Arg::new(id::NORMALIZE)
                .short('N')
                .long("normalize")
                .action(clap::ArgAction::SetTrue)
                .help("Normalize types"),
            clap::Arg::new(id::THEME).long("theme").default_value("ayu").help("Set the theme"),
            clap::Arg::new(id::CAP_LINTS)
                .long("cap-lints")
                .action(clap::ArgAction::SetTrue)
                .help("Cap lints at warning level"),
            clap::Arg::new(id::RUSTC_VERBOSE_INTERNALS)
                .short('#')
                .long("internals")
                .action(clap::ArgAction::SetTrue)
                .help("Enable rustc's `-Zverbose-internals`"),
            clap::Arg::new(id::LOG)
                .long("log")
                .value_name("FILTER")
                .num_args(0..=1)
                .default_missing_value("debug")
                .help("Enable rust{,do}c logging. FILTER defaults to `debug`"),
            clap::Arg::new(id::NO_BACKTRACE)
                .short('B')
                .long("no-backtrace")
                .action(clap::ArgAction::SetTrue)
                .help("Override `RUST_BACKTRACE` to be `0`"),
            clap::Arg::new(id::CROSS_CRATE)
                .short('X')
                .long("cross-crate")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with(id::COMPILETEST)
                .help("Enable the cross-crate re-export mode"),
            // FIXME: The description is not entirely accurate:
            // The flag influences a lot more (edition, crate name obtainment).
            clap::Arg::new(id::COMPILETEST)
                .short('@')
                .long("compiletest")
                .action(clap::ArgAction::SetTrue)
                .help("Enable compiletest directives"),
            clap::Arg::new(id::VERBOSE)
                .short('v')
                .long("verbose")
                .action(clap::ArgAction::SetTrue)
                .help("Use verbose output"),
            clap::Arg::new(id::DRY_RUN)
                .short('0')
                .long("dry-run")
                .action(clap::ArgAction::SetTrue)
                .help("Run through without making any changes"),
            clap::Arg::new(id::COLOR)
                .long("color")
                .value_name("WHEN")
                .default_value("auto")
                .value_parser(clap::builder::EnumValueParser::<ColorChoice>::new())
                .help("Control when to use color"),
        ])
        .get_matches_from(args);

    Arguments {
        toolchain,
        path: matches.remove_one(id::PATH).unwrap(),
        verbatim: dbg!(
            matches.remove_many(id::VERBATIM).map(Iterator::collect).unwrap_or_default()
        ),
        open: matches.remove_one(id::OPEN).unwrap_or_default(),
        crate_name: matches.remove_one(id::CRATE_NAME),
        crate_type: matches.remove_one(id::CRATE_TYPE),
        edition: matches.remove_one(id::EDITION),
        build: BuildFlags {
            cfgs: matches.remove_many(id::CFGS).map(Iterator::collect).unwrap_or_default(),
            revisions: matches
                .remove_many(id::REVISIONS)
                .map(Iterator::collect)
                .unwrap_or_default(),
            cargo_features: matches
                .remove_many(id::CARGO_FEATURES)
                .map(Iterator::collect)
                .unwrap_or_default(),
            rustc_features: matches
                .remove_many(id::RUSTC_FEATURES)
                .map(Iterator::collect)
                .unwrap_or_default(),
            json: matches.remove_one(id::JSON).unwrap_or_default(),
            crate_version: matches.remove_one(id::CRATE_VERSION),
            private: matches.remove_one(id::PRIVATE).unwrap_or_default(),
            hidden: matches.remove_one(id::HIDDEN).unwrap_or_default(),
            layout: matches.remove_one(id::LAYOUT).unwrap_or_default(),
            link_to_definition: matches.remove_one(id::LINK_TO_DEFINITION).unwrap_or_default(),
            normalize: matches.remove_one(id::NORMALIZE).unwrap_or_default(),
            theme: matches.remove_one(id::THEME).unwrap(),
            cap_lints: matches.remove_one(id::CAP_LINTS).unwrap_or_default(),
            rustc_verbose_internals: matches
                .remove_one(id::RUSTC_VERBOSE_INTERNALS)
                .unwrap_or_default(),
            log: matches.remove_one(id::LOG),
            no_backtrace: matches.remove_one(id::NO_BACKTRACE).unwrap_or_default(),
        },
        build_mode: match (
            matches.remove_one(id::CROSS_CRATE).unwrap_or_default(),
            matches.remove_one(id::COMPILETEST).unwrap_or_default(),
        ) {
            (true, false) => BuildMode::CrossCrate,
            (false, true) => BuildMode::Compiletest,
            (false, false) => BuildMode::Default,
            (true, true) => unreachable!(), // Already caught by `clap`.
        },
        debug: DebugFlags {
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
    pub(crate) open: bool,
    pub(crate) crate_name: Option<CrateNameBuf>,
    pub(crate) crate_type: Option<CrateType>,
    pub(crate) edition: Option<Edition>,
    pub(crate) build: BuildFlags,
    pub(crate) build_mode: BuildMode,
    pub(crate) debug: DebugFlags,
    pub(crate) color: ColorChoice,
}

/// Flags that get passed to `rustc` and `rustdoc` in a lowered form.
pub(crate) struct BuildFlags {
    pub(crate) cfgs: Vec<String>,
    pub(crate) revisions: Vec<String>,
    pub(crate) cargo_features: Vec<String>,
    pub(crate) rustc_features: Vec<String>,
    pub(crate) json: bool,
    pub(crate) crate_version: Option<String>,
    pub(crate) private: bool,
    pub(crate) hidden: bool,
    pub(crate) layout: bool,
    pub(crate) link_to_definition: bool,
    pub(crate) normalize: bool,
    pub(crate) theme: String,
    pub(crate) cap_lints: bool,
    pub(crate) rustc_verbose_internals: bool,
    pub(crate) log: Option<String>,
    pub(crate) no_backtrace: bool,
}

pub(crate) struct DebugFlags {
    pub(crate) verbose: bool,
    pub(crate) dry_run: bool,
}

impl Edition {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        match source {
            "D" => Ok(Self::default()),
            "S" => Ok(Self::LATEST_STABLE),
            "U" => Ok(Self::BLEEDING_EDGE),
            source => source.parse(),
        }
        .map_err(|()| possible_values(Self::elements().map(Self::to_str).chain(["D", "S", "U"])))
    }
}

impl CrateNameBuf {
    fn parse_cli_style(source: &str) -> Result<Self, &'static str> {
        Self::adjust_and_parse(source).map_err(|()| "not a non-empty alphanumeric string")
    }
}

impl CrateType {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        source.parse().map_err(|()| possible_values(["bin", "lib", "rlib", "proc-macro"]))
    }
}

fn possible_values(values: impl IntoIterator<Item: std::fmt::Display, IntoIter: Clone>) -> String {
    format!(
        "possible values: {}",
        values.into_iter().map(|value| format!("`{value}`")).join_with(", ")
    )
}

mod id {
    pub(super) const CAP_LINTS: &str = "CAP_LINTS";
    pub(super) const CARGO_FEATURES: &str = "CARGO_FEATURES";
    pub(super) const CFGS: &str = "CFGS";
    pub(super) const COLOR: &str = "COLOR";
    pub(super) const COMPILETEST: &str = "COMPILETEST";
    pub(super) const CRATE_NAME: &str = "CRATE_NAME";
    pub(super) const CRATE_TYPE: &str = "CRATE_TYPE";
    pub(super) const CRATE_VERSION: &str = "CRATE_VERSION";
    pub(super) const CROSS_CRATE: &str = "CROSS_CRATE";
    pub(super) const DRY_RUN: &str = "DRY_RUN";
    pub(super) const EDITION: &str = "EDITION";
    pub(super) const HIDDEN: &str = "HIDDEN";
    pub(super) const JSON: &str = "JSON";
    pub(super) const LAYOUT: &str = "LAYOUT";
    pub(super) const LINK_TO_DEFINITION: &str = "LINK_TO_DEFINITION";
    pub(super) const LOG: &str = "LOG";
    pub(super) const NO_BACKTRACE: &str = "NO_BACKTRACE";
    pub(super) const NORMALIZE: &str = "NORMALIZE";
    pub(super) const OPEN: &str = "OPEN";
    pub(super) const PATH: &str = "PATH";
    pub(super) const PRIVATE: &str = "PRIVATE";
    pub(super) const REVISIONS: &str = "REVISIONS";
    pub(super) const RUSTC_FEATURES: &str = "RUSTC_FEATURES";
    pub(super) const RUSTC_VERBOSE_INTERNALS: &str = "RUSTC_VERBOSE_INTERNALS";
    pub(super) const THEME: &str = "THEME";
    pub(super) const VERBATIM: &str = "VERBATIM";
    pub(super) const VERBOSE: &str = "VERBOSE";
}
