//! The command-line interface.

use crate::{
    build::{BuildOptions, CompileOptions, DebugOptions, DocOptions, Ir, Shallowness},
    data::{
        CrateName, CrateType, DocBackend, Edition, ExtEdition, Identity, PlusPrefixedToolchain,
    },
    directive::Flavor,
    operate::{Bless, CompileMode, DocMode, Open, Operation, Run, Test},
    source::SourcePathBuf,
    utility::{Conjunction, ListingExt as _, default, parse},
};
use std::ffi::OsString;

// Similar to `-h`, `-V` is compatible with all other flags and renders required arguments optional.
// While there could be a world where `-V` is incompatible with flags like `-r` (run) or `-o` (open)
// (i.e., action it prevents from being performed potentially confusing the user), I think it's way
// more convenient for `-V` to have a higher precedence (I can imagine users spontaneously tacking
// `-V` onto a preexisting execution containing `-o` to double check they're using a correctly set up
// toolchain).

pub(crate) fn arguments() -> Arguments {
    let (toolchain, args) = toolchain(std::env::args_os());

    fn source() -> impl IntoIterator<Item = clap::Arg> {
        [
            // The path is intentionally optional to enable invocations like `rrc -V`, `rrc -- -h`,
            // `rrc -- -Zhelp`, `rrc -- -Chelp`, etc.
            clap::Arg::new(id::PATH)
                .value_parser(clap::builder::ValueParser::path_buf())
                .help("Path to the source file"),
            clap::Arg::new(id::SOURCE)
                .short(':')
                .long("source")
                .conflicts_with(id::PATH)
                .help("Provide the source code"),
            clap::Arg::new(id::extern_)
                .short('x')
                .long("extern")
                .value_name("PATH")
                .value_parser(clap::builder::ValueParser::path_buf())
                .action(clap::ArgAction::Append)
                // FIXME: Temporary limitation of the operation module.
                .conflicts_with(id::directives)
                .help("Add the source file path to an extern crate"),
        ]
    }
    fn verbatim() -> clap::Arg {
        clap::Arg::new(id::verbatim).num_args(..).last(true).value_name("VERBATIM")
    }
    fn compiletest() -> impl IntoIterator<Item = clap::Arg> {
        [
            clap::Arg::new(id::directives)
                .short('@')
                .long("directives")
                .value_name("FLAVOR")
                .require_equals(true)
                .num_args(..=1)
                .default_missing_value("vanilla")
                .value_parser(Flavor::parse_cli_style)
                .help("Enable compiletest-like directives"),
            clap::Arg::new(id::compiletest)
                .short('T')
                .long("compiletest")
                .action(clap::ArgAction::SetTrue)
                // FIXME: Maybe reject if flavor isn't vanilla (`-@=x`)?
                .requires(id::directives)
                .help("Check in a compiletest-esque manner"),
            clap::Arg::new(id::bless)
                .short('.')
                .long("bless")
                .requires(id::compiletest)
                .action(clap::ArgAction::SetTrue)
                .help("Update the test expectations"),
        ]
    }
    fn crate_name_and_type() -> impl IntoIterator<Item = clap::Arg> {
        [
            clap::Arg::new(id::crate_name)
                .short('n')
                .long("crate-name")
                .value_name("NAME")
                .value_parser(CrateName::parse_cli_style)
                .help("Set the name of the crate"),
            clap::Arg::new(id::crate_type)
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
            clap::Arg::new(id::cfgs)
                .long("cfg")
                // FIXME: This gets rendered as `<NAME[="VALUE"]>` by clap but ideally we'd print `<NAME>[="<VALUE>"]`.
                .value_name(r#"NAME[="VALUE"]"#)
                .action(clap::ArgAction::Append)
                .help("Enable a configuration"),
            clap::Arg::new(id::revision)
                .short('R')
                .long("revision")
                .value_name("NAME")
                .requires(id::directives)
                .help("Enable a compiletest revision"),
            // FIXME: This doesn't really belong in this "group" (`cfgs`)
            clap::Arg::new(id::unstable_features)
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
            clap::Arg::new(id::suppress_lints)
                .short('/')
                .long("suppress-lints")
                .action(clap::ArgAction::SetTrue)
                .help("Cap lints at allow level"),
            clap::Arg::new(id::internals)
                .short('#')
                .long("internals")
                .action(clap::ArgAction::SetTrue)
                .help("Enable internal pretty-printing of data types"),
            clap::Arg::new(id::next_solver)
                .short('N')
                .long("next-solver")
                .action(clap::ArgAction::SetTrue)
                .help("Enable the next-gen trait solver"),
            clap::Arg::new(id::identity)
                .short('I')
                .long("identity")
                .value_name("IDENTITY")
                .value_parser(Identity::parse_cli_style)
                .help("Force rust{,do}c's identity"),
            // FIXME: Does this actually work for rustdoc?
            clap::Arg::new(id::no_dedupe)
                .short('D')
                .long("no-dedupe")
                .action(clap::ArgAction::SetTrue)
                .help("Don't deduplicate diagnostics"),
            clap::Arg::new(id::log)
                .long("log")
                .value_name("FILTER")
                .require_equals(true)
                .num_args(..=1)
                .default_missing_value("debug")
                .help("Enable rust{,do}c logging. FILTER defaults to `debug`"),
            clap::Arg::new(id::no_backtrace)
                .short('B')
                .long("no-backtrace")
                .action(clap::ArgAction::SetTrue)
                .help("Override `RUST_BACKTRACE` to be `0`"),
            clap::Arg::new(id::print_engine_version)
                .short('V')
                .long("version")
                .action(clap::ArgAction::SetTrue)
                .help("Print the underlying rust{,do}c version and halt"),
            clap::Arg::new(id::verbose)
                .short('v')
                .long("verbose")
                .action(clap::ArgAction::SetTrue)
                .help("Use verbose output"),
            clap::Arg::new(id::color)
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
            clap::Command::new(id::build)
                .alias("b")
                .about("Compile the given crate with rustc")
                .defer(|command| {
                    command
                        .args(source())
                        .arg(verbatim().help("Flags passed to `rustc` verbatim"))
                        .arg(
                            clap::Arg::new(id::run)
                                .short('r')
                                .long("run")
                                .action(clap::ArgAction::SetTrue)
                                .conflicts_with(id::compiletest)
                                .help("Also run the built binary"),
                        )
                        .arg(
                            clap::Arg::new(id::check_only)
                                .short('c')
                                .long("check-only")
                                .action(clap::ArgAction::SetTrue)
                                .conflicts_with(id::run)
                                .help("Don't fully compile, only check the crate"),
                        )
                        .args(compiletest())
                        .args(crate_name_and_type())
                        .arg(edition())
                        .args(cfgs())
                        .arg(
                            clap::Arg::new(id::shallow)
                                .short('s')
                                .long("shallow")
                                .value_name("MODE")
                                .require_equals(true)
                                .num_args(..=1)
                                .default_missing_value("parse-only")
                                .value_parser(Shallowness::parse_cli_style)
                                // FIXME: W/ the intro of `-s=cfg-false` not quite accurate anymore
                                .help("Halt after parsing the source file")
                                .conflicts_with(id::run),
                        )
                        .arg(
                            clap::Arg::new(id::dump)
                                .short('d')
                                .long("dump")
                                .value_name("IR")
                                .value_parser(Ir::parse_cli_style)
                                .help("Print the given compiler IR"),
                        )
                        .args(extra())
                }),
            clap::Command::new(id::doc)
                .alias("d")
                .about("Document the given crate with rustdoc")
                .defer(|command| {
                    command
                        .args(source())
                        .arg(verbatim().help("Flags passed to `rustc` and `rustdoc` verbatim"))
                        .arg(
                            clap::Arg::new(id::open)
                                .short('o')
                                .long("open")
                                .action(clap::ArgAction::SetTrue)
                                .conflicts_with(id::compiletest)
                                .help("Also open the generated docs in a browser"),
                        )
                        .arg(
                            clap::Arg::new(id::json)
                                .short('j')
                                .long("json")
                                .conflicts_with(id::open)
                                .action(clap::ArgAction::SetTrue)
                                .help("Output JSON instead of HTML"),
                        )
                        .args(compiletest())
                        .arg(
                            clap::Arg::new(id::cross_crate)
                                .short('X')
                                .long("cross-crate")
                                .action(clap::ArgAction::SetTrue)
                                .conflicts_with(id::directives)
                                .help("Enable the cross-crate re-export mode"),
                        )
                        .args(crate_name_and_type())
                        .arg(
                            clap::Arg::new(id::crate_version)
                                .long("crate-version")
                                .value_name("VERSION")
                                .help("Set the version of the (base) crate"),
                        )
                        .arg(edition())
                        .args(cfgs())
                        .args([
                            clap::Arg::new(id::private)
                                .short('P')
                                .long("private")
                                .action(clap::ArgAction::SetTrue)
                                .help("Document private items"),
                            clap::Arg::new(id::hidden)
                                .short('H')
                                .long("hidden")
                                .action(clap::ArgAction::SetTrue)
                                .help("Document hidden items"),
                            clap::Arg::new(id::layout)
                                .long("layout")
                                .action(clap::ArgAction::SetTrue)
                                .help("Document the memory layout of types"),
                            clap::Arg::new(id::link_to_def)
                                .long("link-to-def")
                                .alias("ltd")
                                .action(clap::ArgAction::SetTrue)
                                .help("Generate links to definitions"),
                            clap::Arg::new(id::normalize)
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

    let directives = matches.remove_one::<Flavor>(id::directives).map(|flavor| {
        crate::operate::DirectiveOptions {
            flavor,
            revision: matches.remove_one(id::revision),
            test: match matches.remove_one(id::compiletest).unwrap_or_default() {
                false => Test::No,
                true => Test::Yes(match matches.remove_one(id::bless).unwrap_or_default() {
                    false => Bless::No,
                    true => Bless::Yes,
                }),
            },
        }
    });

    let print_engine_version: bool =
        matches.remove_one(id::print_engine_version).unwrap_or_default();

    let operation = match (operation.as_str(), print_engine_version) {
        (id::build, false) => Operation::Compile {
            run: match matches.remove_one::<bool>(id::run).unwrap_or_default() {
                true => Run::Yes,
                false => Run::No,
            },
            mode: match directives {
                Some(dir_opts) => CompileMode::DirectiveDriven(dir_opts),
                None => CompileMode::Default,
            },
            options: CompileOptions {
                check_only: matches.remove_one(id::check_only).unwrap_or_default(),
                shallowness: matches.remove_one(id::shallow),
                dump: matches.remove_one(id::dump),
            },
        },
        (id::build, true) => Operation::QueryRustcVersion,
        (id::doc, false) => Operation::Document {
            open: match matches.remove_one::<bool>(id::open).unwrap_or_default() {
                true => Open::Yes,
                false => Open::No,
            },
            mode: match (matches.remove_one(id::cross_crate).unwrap_or_default(), directives) {
                (true, None) => DocMode::CrossCrate,
                (false, Some(dir_opts)) => DocMode::DirectiveDriven(dir_opts),
                (false, None) => DocMode::Default,
                (true, Some(_)) => unreachable!(), // Already caught by `clap`.
            },
            options: DocOptions {
                backend: if matches.remove_one(id::json).unwrap_or_default() {
                    DocBackend::Json
                } else {
                    DocBackend::Html
                },
                crate_version: matches.remove_one(id::crate_version),
                private: matches.remove_one(id::private).unwrap_or_default(),
                hidden: matches.remove_one(id::hidden).unwrap_or_default(),
                layout: matches.remove_one(id::layout).unwrap_or_default(),
                link_to_def: matches.remove_one(id::link_to_def).unwrap_or_default(),
                normalize: matches.remove_one(id::normalize).unwrap_or_default(),
                theme: matches.remove_one(id::THEME).unwrap(),
                v_opts: default(),
            },
        },
        (id::doc, true) => Operation::QueryRustdocVersion,
        _ => unreachable!(), // handled by `clap`,
    };

    // FIXME: Don't leak the crate type and the edition!
    //        Sadly, clap doesn't support zero-copy deserialization /
    //        deserializing from borrowed program arguments and providing &strs.
    //        Fix: Throw out clap and do it manually.

    let source = matches.remove_one(id::SOURCE).map(Source::String);
    let path = matches.remove_one(id::PATH).map(SourcePathBuf::new);
    let source = source.xor(path.map(Source::Path));

    Arguments {
        toolchain,
        source,
        dependencies: matches
            .remove_many(id::extern_)
            .map(|paths| paths.into_iter().map(SourcePathBuf::new).collect())
            .unwrap_or_default(),
        verbatim: matches.remove_many(id::verbatim).map(Iterator::collect).unwrap_or_default(),
        operation,
        crate_name: matches.remove_one(id::crate_name),
        crate_type: matches
            .remove_one(id::crate_type)
            .map(|typ: String| CrateType::parse_cli_style(typ.leak())),
        edition: matches
            .remove_one(id::EDITION)
            .map(|edition: String| ExtEdition::parse_cli_style(edition.leak())),
        b_opts: BuildOptions {
            cfgs: matches.remove_many(id::cfgs).map(Iterator::collect).unwrap_or_default(),
            unstable_features: matches
                .remove_many(id::unstable_features)
                .map(Iterator::collect)
                .unwrap_or_default(),
            extern_crates: default(),
            suppress_lints: matches.remove_one(id::suppress_lints).unwrap_or_default(),
            internals: matches.remove_one(id::internals).unwrap_or_default(),
            next_solver: matches.remove_one(id::next_solver).unwrap_or_default(),
            identity: matches.remove_one(id::identity),
            no_dedupe: matches.remove_one(id::no_dedupe).unwrap_or_default(),
            log: matches.remove_one(id::log),
            no_backtrace: matches.remove_one(id::no_backtrace).unwrap_or_default(),
        },
        dbg_opts: DebugOptions { verbose: matches.remove_one(id::verbose).unwrap() },
        color: matches.remove_one(id::color).unwrap(),
    }
}

fn toolchain(mut args: std::env::ArgsOs) -> (Option<PlusPrefixedToolchain>, Vec<OsString>) {
    // FIXME: Ideally, `clap` would support custom prefixes (for positional args), here: `+`.
    //        However, it does not. See also <https://github.com/clap-rs/clap/issues/2468>.
    //        Therefore, we need to extract it ourselves.
    // FIXME: It would be nice if we could show this as `[+<TOOLCHAIN>]` or similar in the help output.

    let (capacity, _) = args.size_hint();
    let mut result = Vec::with_capacity(capacity);

    if let Some(bin) = args.next() {
        result.push(bin);
    }
    if let Some(subcommand) = args.next() {
        // FIXME: If this resembles a toolchain argument, emit a custom
        //        error suggesting to move it after the subcommand.
        result.push(subcommand);
    }

    let toolchain = args
        .next()
        .and_then(|arg| PlusPrefixedToolchain::new(arg).map_err(|arg| result.push(arg)).ok());

    result.extend(args);

    (toolchain, result)
}

pub(crate) struct Arguments {
    pub(crate) toolchain: Option<PlusPrefixedToolchain>,
    pub(crate) source: Option<Source>,
    pub(crate) dependencies: Vec<SourcePathBuf>,
    pub(crate) verbatim: Vec<String>,
    pub(crate) operation: Operation,
    pub(crate) crate_name: Option<CrateName<String>>,
    pub(crate) crate_type: Option<CrateType>,
    pub(crate) edition: Option<ExtEdition<'static>>,
    pub(crate) b_opts: BuildOptions,
    pub(crate) dbg_opts: DebugOptions,
    pub(crate) color: clap::ColorChoice,
}

pub(crate) enum Source {
    Path(SourcePathBuf),
    String(String),
}

impl ExtEdition<'static> {
    // FIXME: Take `<'a> &'a str` once clap is thrown out.
    // FIXME: Somehow support `h`/`help` printing out rrx's superset of options
    fn parse_cli_style(source: &'static str) -> Self {
        Self::Fixed(match source {
            "d" | "default" => return Self::EngineDefault,
            "s" | "stable" => return Self::LatestStable,
            "u" | "unstable" => return Self::LatestUnstable,
            "l" | "latest" => return Self::Latest,
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
        Self::parse_relaxed(source).map_err(|()| "not a non-empty alphanumeric string")
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
            "t" | "true" => Self::True,
            "s" | "stable" => Self::Stable,
            "n" | "nightly" => Self::Nightly,
        )(source)
        .map_err(possible_values)
    }
}

impl Flavor {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        parse!(
            "v" | "vanilla" => Self::Vanilla,
            "x" | "rruxwry" => Self::Rruxwry,
        )(source)
        .map_err(possible_values)
    }
}

impl Shallowness {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        parse!(
            "#" | "cfg-false" => Self::CfgFalse,
            "parse-only" => Self::ParseOnly,
        )(source)
        .map_err(possible_values)
    }
}

impl Ir {
    fn parse_cli_style(source: &str) -> Result<Self, String> {
        parse!(
            "ast" => Self::Ast,
            "astpp" => Self::Astpp,
            "xast" => Self::Xast,
            "xastpp" => Self::Xastpp,
            "hir" => Self::Hir,
            "hirpp" => Self::Hirpp,
            "thir" => Self::Thir,
            "mir" => Self::Mir,
            "lir" => Self::Lir,
            "asm" => Self::Asm,
        )(source)
        .map_err(possible_values)
    }
}

// FIXME: clap requires the ret ty to be ~owned, ideally we'd just return `&'input str`.
#[expect(clippy::unnecessary_wraps)] // not in our control
fn parse_unstable_feature_cli_style(source: &str) -> Result<String, String> {
    Ok(match source {
        "ace" => "associated_const_equality",
        "acp" | "adt" => "adt_const_params",
        "afidt" => "async_fn_in_dyn_trait",
        "ast" | "selfx" => "arbitrary_self_types",
        "at" | "auto" | "auto_trait" => "auto_traits",
        "atd" => "associated_type_defaults",
        "bs" | "builtin" => "builtin_syntax",
        "cia" => "custom_inner_attributes",
        "clb" => "closure_lifetime_binder",
        "co" => "coroutines",
        "cti" | "const" | "~" => "const_trait_impl",
        "dm" | "m" | "macro" => "decl_macro",
        "dp" | "deref" => "deref_patterns",
        "eii" => "extern_item_impls",
        "et" | "extern" => "extern_types",
        "faf" | "final" => "final_associated_functions",
        "fd" | "del" | "reuse" => "fn_delegation",
        "fp" => "field_projections",
        "frtr" | "frt" => "field_representing_type_raw",
        "gb" | "gen" => "gen_blocks",
        "gce" => "generic_const_exprs",
        "gci" => "generic_const_items",
        "gcpt" | "gcg" => "generic_const_parameter_types",
        "gpt" | "ptx" | "patx" => "generic_pattern_types",
        "iat" => "inherent_associated_types",
        "itaf" => "import_trait_associated_functions",
        "itiat" | "atpit" => "impl_trait_in_assoc_type",
        "itib" => "impl_trait_in_bindings",
        "itiftr" => "impl_trait_in_fn_trait_return",
        "li" | "l" => "lang_items",
        "lta" => "lazy_type_alias",
        "marker" => "marker_trait_attr",
        "mgca" => "min_generic_const_args",
        "minspec" | "mspec" => "min_specialization",
        "mmb" | "relaxed" | "?" => "more_maybe_bounds",
        "mme" | "meta" | "$" => "macro_metavar_expr",
        "mmec" => "macro_metavar_expr_concat",
        "mqp" | "struct" => "more_qualified_paths",
        "nb" => "negative_bounds",
        "ni" => "negative_impls",
        "nlb" | "binders" | "for" => "non_lifetime_binders",
        "np" => "never_patterns",
        "nt" | "n" | "never" | "!" => "never_type",
        "ogca" => "opaque_generic_const_args",
        "pt" | "pat" | "pattern" => "pattern_types",
        "ra" | "a" | "attrs" | "#" | "rustc_attr" => "rustc_attrs",
        "rtn" => "return_type_notation",
        "sa" | "api" | "stable" | "unstable" => "staged_api",
        "sea" => "stmt_expr_attributes",
        "sh" | "sized" => "sized_hierarchy",
        "spec" => "specialization",
        "ta" | "trait_aliases" => "trait_alias",
        "tait" => "type_alias_impl_trait",
        "tb" | "trivial" => "trivial_bounds",
        "tcsu" | "update" => "type_changing_struct_update",
        "try" => "try_blocks",
        "uc" | "fn" => "unboxed_closures",
        "ucp" => "unsized_const_params",
        "wca" | "where" => "where_clause_attrs",
        "wnc" | "nc" => "with_negative_coherence",
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

macro_rules! ids {
    ($($id:ident),+ $(,)?) => {
        #[allow(non_upper_case_globals)]
        mod id {
            $( pub(super) const $id: &str = stringify!($id); )+
        }
    };
}

#[rustfmt::skip]
ids! {
    bless, build, cfgs, check_only, color, compiletest, crate_name, crate_type, crate_version,
    cross_crate, directives, doc, dump, EDITION, extern_, hidden, identity, internals,
    json, layout, link_to_def, log, next_solver, normalize, no_backtrace, no_dedupe, open,
    PATH, print_engine_version, private, revision, run, shallow, SOURCE, suppress_lints, THEME,
    unstable_features, verbatim, verbose,
}
