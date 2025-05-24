//! The command-line interface.

use crate::{
    build::{BuildOptions, CompileOptions, DebugOptions, DocOptions},
    data::{CrateName, CrateType, DocBackend, Edition, ExtEdition, Identity},
    directive::Flavor,
    operate::{Bless, CompileMode, DocMode, Open, Operation, Run, Test},
    utility::{Conjunction, ListingExt as _, default, parse},
};
use std::{ffi::OsString, path::PathBuf};

// Similar to `-h`, `-Q` is compatible with all other flags and renders required arguments optional.
// While there could be a world where `-Q` is incompatible with flags like `-r` (run) or `-o` (open)
// (i.e., action it prevents from being performed potentially confusing the user), I think it's way
// more convenient for `-Q` to have a higher precedence (I can imagine users spontaneously tacking
// `-Q` onto a preexisting execution containing `-o` to double check they're using a correctly set up
// toolchain).

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
        // The path is intentionally optional to enable invocations like `rrc -V`, `rrc -- -h`,
        // `rrc -- -Zhelp`, `rrc -- -Chelp`, etc.
        clap::Arg::new(id::PATH)
            .value_parser(clap::builder::ValueParser::path_buf())
            .help("Path to the source file")
    }
    fn verbatim() -> clap::Arg {
        clap::Arg::new(id::VERBATIM).num_args(..).last(true).value_name("VERBATIM")
    }
    fn compiletest() -> impl IntoIterator<Item = clap::Arg> {
        [
            // FIXME: Ideally the long form for Flavor::Rruxwry wasn't
            //        `--directives --directives` (yuck!) but sth. like
            //        `--directives=rruxwry` while the short form remains `-@@`!
            clap::Arg::new(id::DIRECTIVES)
                .short('@')
                .long("directives")
                // FIXME: Limit number of occurrences to 0..=2 (`max_occurences` no longer exists).
                .action(clap::ArgAction::Count)
                .help("Enable compiletest-like directives"),
            // FIXME: (Reminder) Warn on `--bless`+`--dry-run` (outside of clap)
            clap::Arg::new(id::COMPILETEST)
                .short('T')
                .long("compiletest")
                .action(clap::ArgAction::SetTrue)
                // FIXME: Requires -@ but incompatible with -@@ etc!
                .requires(id::DIRECTIVES)
                .help("Check in a compiletest-esque manner"),
            clap::Arg::new(id::BLESS)
                .short('.')
                .long("bless")
                .requires(id::COMPILETEST)
                .action(clap::ArgAction::SetTrue)
                .help("Update the test expectations"),
        ]
    }
    fn crate_name_and_type() -> impl IntoIterator<Item = clap::Arg> {
        [
            clap::Arg::new(id::CRATE_NAME)
                .short('n')
                .long("crate-name")
                .value_name("NAME")
                .value_parser(CrateName::parse_cli_style)
                .help("Set the name of the crate"),
            clap::Arg::new(id::CRATE_TYPE)
                .short('t')
                .long("crate-type")
                .value_name("TYPE")
                .help("Set the type of the crate"),
        ]
    }
    fn edition() -> clap::Arg {
        clap::Arg::new(id::EDITION).short('e').long("edition").help("Set the edition of the crate")
    }
    fn cfgs() -> impl IntoIterator<Item = clap::Arg> {
        [
            clap::Arg::new(id::CFGS)
                .long("cfg")
                // FIXME: This gets rendered as `<NAME[="VALUE"]>` by clap but ideally we'd print `<NAME>[="<VALUE>"]`.
                .value_name(r#"NAME[="VALUE"]"#)
                .action(clap::ArgAction::Append)
                .help("Enable a configuration"),
            clap::Arg::new(id::REVISION)
                .short('R')
                .long("revision")
                .value_name("NAME")
                .requires(id::DIRECTIVES)
                .help("Enable a compiletest revision"),
            // FIXME: This doesn't really belong in this "group" (`cfgs`)
            clap::Arg::new(id::UNSTABLE_FEATURES)
                .short('F')
                .long("feature")
                .value_name("NAME")
                .value_parser(parse_unstable_feature_cli_style)
                .action(clap::ArgAction::Append)
                .help("Enable an experimental library or language feature"),
        ]
    }
    fn extra() -> impl IntoIterator<Item = clap::Arg> {
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
                .help("Enable internal pretty-printing of data types"),
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
            clap::Arg::new(id::NO_DEDUPE)
                .short('D')
                .long("no-dedupe")
                .action(clap::ArgAction::SetTrue)
                .help("Don't deduplicate diagnostics"),
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
            clap::Arg::new(id::ENGINE_VERSION)
                .short('V')
                .long("version")
                .action(clap::ArgAction::SetTrue)
                .help("Print the underlying rust{,do}c version and halt"),
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
                .value_parser(clap::builder::EnumValueParser::<clap::ColorChoice>::new())
                .help("Control when to use color"),
        ]
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
                                .conflicts_with(id::COMPILETEST)
                                .help("Also run the built binary"),
                        )
                        .arg(
                            clap::Arg::new(id::CHECK_ONLY)
                                .short('c')
                                .long("check-only")
                                .action(clap::ArgAction::SetTrue)
                                .conflicts_with(id::RUN)
                                .help("Don't fully compile, only check the crate"),
                        )
                        .args(compiletest())
                        .args(crate_name_and_type())
                        .arg(edition())
                        .args(cfgs())
                        .arg(
                            clap::Arg::new(id::SHALLOW)
                                .short('s')
                                .long("shallow")
                                .action(clap::ArgAction::SetTrue)
                                .help("Halt after parsing the source file")
                                .conflicts_with(id::RUN),
                        )
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
                                .conflicts_with(id::COMPILETEST)
                                .help("Also open the generated docs in a browser"),
                        )
                        .arg(
                            clap::Arg::new(id::JSON)
                                .short('j')
                                .long("json")
                                .conflicts_with(id::OPEN)
                                .action(clap::ArgAction::SetTrue)
                                .help("Output JSON instead of HTML"),
                        )
                        .args(compiletest())
                        .arg(
                            clap::Arg::new(id::CROSS_CRATE)
                                .short('X')
                                .long("cross-crate")
                                .action(clap::ArgAction::SetTrue)
                                .conflicts_with(id::DIRECTIVES)
                                .help("Enable the cross-crate re-export mode"),
                        )
                        .args(crate_name_and_type())
                        .arg(
                            clap::Arg::new(id::CRATE_VERSION)
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

    let directives = match matches.remove_one::<u8>(id::DIRECTIVES).unwrap_or_default() {
        0 => None,
        1 => Some(Flavor::Vanilla),
        // FIXME: Reject count > 2.
        _ => Some(Flavor::Rruxwry),
    };

    let test = match matches.remove_one(id::COMPILETEST).unwrap_or_default() {
        false => Test::No,
        true => Test::Yes(match matches.remove_one(id::BLESS).unwrap_or_default() {
            false => Bless::No,
            true => Bless::Yes,
        }),
    };

    let query_engine_version: bool = matches.remove_one(id::ENGINE_VERSION).unwrap_or_default();

    let operation = match (operation.as_str(), query_engine_version) {
        (id::BUILD, false) => Operation::Compile {
            run: match matches.remove_one::<bool>(id::RUN).unwrap_or_default() {
                true => Run::Yes,
                false => Run::No,
            },
            mode: match directives {
                Some(flavor) => CompileMode::DirectiveDriven(flavor, test),
                None => CompileMode::Default,
            },
            options: CompileOptions {
                check_only: matches.remove_one(id::CHECK_ONLY).unwrap_or_default(),
                shallow: matches.remove_one(id::SHALLOW).unwrap_or_default(),
            },
        },
        (id::BUILD, true) => Operation::QueryRustcVersion,
        (id::DOC, false) => Operation::Document {
            open: match matches.remove_one::<bool>(id::OPEN).unwrap_or_default() {
                true => Open::Yes,
                false => Open::No,
            },
            mode: match (matches.remove_one(id::CROSS_CRATE).unwrap_or_default(), directives) {
                (true, None) => DocMode::CrossCrate,
                (false, Some(flavor)) => DocMode::DirectiveDriven(flavor, test),
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
                v_opts: default(),
            },
        },
        (id::DOC, true) => Operation::QueryRustdocVersion,
        _ => unreachable!(), // handled by `clap`,
    };

    // FIXME: Don't leak the crate type and the edition!
    //        Sadly, clap doesn't support zero-copy deserialization /
    //        deserializing from borrowed program arguments and providing &strs.
    //        Fix: Throw out clap and do it manually.
    Arguments {
        toolchain,
        path: matches.remove_one(id::PATH),
        verbatim: matches.remove_many(id::VERBATIM).map(Iterator::collect).unwrap_or_default(),
        operation,
        crate_name: matches.remove_one(id::CRATE_NAME),
        crate_type: matches
            .remove_one(id::CRATE_TYPE)
            .map(|typ: String| CrateType::parse_cli_style(typ.leak())),
        edition: matches
            .remove_one(id::EDITION)
            .map(|edition: String| ExtEdition::parse_cli_style(edition.leak())),
        b_opts: BuildOptions {
            cfgs: matches.remove_many(id::CFGS).map(Iterator::collect).unwrap_or_default(),
            revision: matches.remove_one(id::REVISION),
            unstable_features: matches
                .remove_many(id::UNSTABLE_FEATURES)
                .map(Iterator::collect)
                .unwrap_or_default(),
            suppress_lints: matches.remove_one(id::SUPPRESS_LINTS).unwrap_or_default(),
            internals: matches.remove_one(id::INTERNALS).unwrap_or_default(),
            next_solver: matches.remove_one(id::NEXT_SOLVER).unwrap_or_default(),
            identity: matches.remove_one(id::IDENTITY),
            no_dedupe: matches.remove_one(id::NO_DEDUPE).unwrap_or_default(),
            log: matches.remove_one(id::LOG),
            no_backtrace: matches.remove_one(id::NO_BACKTRACE).unwrap_or_default(),
        },
        dbg_opts: DebugOptions {
            verbose: matches.remove_one(id::VERBOSE).unwrap(),
            dry_run: matches.remove_one(id::DRY_RUN).unwrap(),
        },
        color: matches.remove_one(id::COLOR).unwrap(),
    }
}

pub(crate) struct Arguments {
    /// The toolchain, prefixed with `+`.
    pub(crate) toolchain: Option<OsString>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) verbatim: Vec<String>,
    pub(crate) operation: Operation,
    pub(crate) crate_name: Option<CrateName<String>>,
    pub(crate) crate_type: Option<CrateType>,
    pub(crate) edition: Option<ExtEdition<'static>>,
    pub(crate) b_opts: BuildOptions,
    pub(crate) dbg_opts: DebugOptions,
    pub(crate) color: clap::ColorChoice,
}

impl ExtEdition<'static> {
    // FIXME: Take `<'a> &'a str` once clap is thrown out.
    // FIXME: Somehow support `h`/`help` printing out rrx's superset of options
    fn parse_cli_style(source: &'static str) -> Self {
        Self::Fixed(match source {
            "d" => return Self::EngineDefault,
            "s" => return Self::LatestStable,
            "u" => return Self::LatestUnstable,
            "l" => return Self::Latest,
            "15" | "2015" => Edition::Rust2015,
            "18" | "2018" => Edition::Rust2018,
            "21" | "2021" => Edition::Rust2021,
            "24" | "2024" => Edition::Rust2024,
            "f" | "future" => Edition::Future,
            _ => Edition::Unknown(source),
        })
    }
}

impl CrateName<String> {
    fn parse_cli_style(source: &str) -> Result<Self, &'static str> {
        Self::adjust_and_parse(source).map_err(|()| "not a non-empty alphanumeric string")
    }
}

impl CrateType {
    // FIXME: Take <'a> &'a str string once clap is thrown out.
    fn parse_cli_style(source: &'static str) -> Self {
        match source {
            "b" => Self("bin"),
            "l" => Self("lib"),
            "m" => Self("proc-macro"),
            _ => Self(source),
        }
    }
}

impl Identity {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        parse!(
            "t" => Self::True,
            "s" => Self::Stable,
            "n" => Self::Nightly,
        )(source)
        .map_err(possible_values)
    }
}

// FIXME: clap requires the ret ty to be ~owned, ideally we'd just return `&'input str`.
fn parse_unstable_feature_cli_style(source: &str) -> Result<String, String> {
    Ok(match source {
        "ace" => "associated_const_equality",
        "acp" => "adt_const_params",
        "dm" | "m" => "decl_macro",
        "gce" => "generic_const_exprs",
        "gci" => "generic_const_items",
        "gcpt" | "gcg" => "generic_const_parameter_types",
        "iat" => "inherent_associated_types",
        "itiat" | "atpit" => "impl_trait_in_assoc_type",
        "itib" => "impl_trait_in_bindings",
        "lta" => "lazy_type_alias",
        "mgca" => "min_generic_const_args",
        "nlb" => "non_lifetime_binders",
        "rtn" => "return_type_notation",
        "sea" => "stmt_expr_attributes",
        "ta" => "trait_alias",
        "tait" => "type_alias_impl_trait",
        "tcsu" => "type_changing_struct_update",
        "ucp" => "unsized_const_params",
        _ => source,
    }
    .to_string())
}

fn possible_values(values: impl Iterator<Item: std::fmt::Display> + Clone) -> String {
    format!(
        "possible values: {}",
        values.into_iter().map(|value| format!("`{value}`")).list(Conjunction::Or)
    )
}

mod id {
    pub(super) const BLESS: &str = "BLESS";
    pub(super) const BUILD: &str = "build";
    pub(super) const CFGS: &str = "CFGS";
    pub(super) const CHECK_ONLY: &str = "CHECK_ONLY";
    pub(super) const COLOR: &str = "COLOR";
    pub(super) const COMPILETEST: &str = "COMPILETEST";
    pub(super) const CRATE_NAME: &str = "CRATE_NAME";
    pub(super) const CRATE_TYPE: &str = "CRATE_TYPE";
    pub(super) const CRATE_VERSION: &str = "CRATE_VERSION";
    pub(super) const CROSS_CRATE: &str = "CROSS_CRATE";
    pub(super) const DIRECTIVES: &str = "DIRECTIVES";
    pub(super) const DOC: &str = "doc";
    pub(super) const DRY_RUN: &str = "DRY_RUN";
    pub(super) const EDITION: &str = "EDITION";
    pub(super) const ENGINE_VERSION: &str = "ENGINE_VERSION";
    pub(super) const HIDDEN: &str = "HIDDEN";
    pub(super) const IDENTITY: &str = "IDENTITY";
    pub(super) const INTERNALS: &str = "INTERNALS";
    pub(super) const JSON: &str = "JSON";
    pub(super) const LAYOUT: &str = "LAYOUT";
    pub(super) const LINK_TO_DEF: &str = "LINK_TO_DEF";
    pub(super) const LOG: &str = "LOG";
    pub(super) const NEXT_SOLVER: &str = "NEXT_SOLVER";
    pub(super) const NO_BACKTRACE: &str = "NO_BACKTRACE";
    pub(super) const NO_DEDUPE: &str = "NO_DEDUPE";
    pub(super) const NORMALIZE: &str = "NORMALIZE";
    pub(super) const OPEN: &str = "OPEN";
    pub(super) const PATH: &str = "PATH";
    pub(super) const PRIVATE: &str = "PRIVATE";
    pub(super) const REVISION: &str = "REVISION";
    pub(super) const RUN: &str = "RUN";
    pub(super) const SHALLOW: &str = "SHALLOW";
    pub(super) const SUPPRESS_LINTS: &str = "SUPPRESS_LINTS";
    pub(super) const THEME: &str = "THEME";
    pub(super) const UNSTABLE_FEATURES: &str = "UNSTABLE_FEATURES";
    pub(super) const VERBATIM: &str = "VERBATIM";
    pub(super) const VERBOSE: &str = "VERBOSE";
}
