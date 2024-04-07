//! The command-line interface.

use crate::data::{CrateNameBuf, CrateType, Edition, LintLevel};
use clap::{
    builder::{PossibleValue, TypedValueParser},
    error::ErrorKind,
    Arg, ColorChoice, Command, Error, Parser, ValueEnum,
};
use std::{ffi::OsStr, path::PathBuf};

#[derive(Parser)]
#[command(about)]
pub(crate) struct Arguments {
    /// Path to the source file.
    pub(crate) path: PathBuf,
    /// Flags passed to `rustc` and `rustdoc` verbatim.
    #[arg(last(true), value_name("VERBATIM"))]
    pub(crate) verbatim_flags: Vec<String>,
    /// Open the generated docs in a browser.
    #[arg(short, long)]
    pub(crate) open: bool,
    /// Set the name of the (base) crate.
    #[arg(short = 'n', long, value_name("NAME"), value_parser = CrateNameParser)]
    pub(crate) crate_name: Option<CrateNameBuf>,
    /// Set the type of the (base) crate.
    #[arg(short = 'y', long, value_name("TYPE"), value_parser = CrateTypeParser)]
    pub(crate) crate_type: Option<CrateType>,
    /// Set the edition of the source files.
    #[arg(short, long, value_enum)]
    pub(crate) edition: Option<Edition>,
    #[command(flatten)]
    pub(crate) build_flags: BuildFlags,
    /// Enable the cross-crate re-export mode.
    #[arg(short = 'X', long)]
    pub(crate) cross_crate: bool,
    /// Enable ui_test-style compiletest directives: `//@`.
    #[arg(short = 'T', long, conflicts_with("cross_crate"))]
    pub(crate) compiletest: bool,
    /// Enable XPath / JsonPath queries.
    #[arg(
        short = 'Q',
        long,
        conflicts_with("cross_crate"),
        requires("compiletest")
    )]
    pub(crate) query: bool,
    #[command(flatten)]
    pub(crate) program_flags: ProgramFlags,
    /// Control when to use color.
    #[arg(long, value_name("WHEN"), default_value("auto"))]
    pub(crate) color: ColorChoice,
}

/// Flags that get passed to `rustc` and `rustdoc` in a lowered form.
#[derive(Parser)]
pub(crate) struct BuildFlags {
    /// Set the toolchain.
    #[arg(short, long, value_name("NAME"))]
    pub(crate) toolchain: Option<String>,
    /// Enable a `cfg`.
    #[arg(long = "cfg", value_name("SPEC"))]
    pub(crate) cfgs: Vec<String>,
    /// Enable a compiletest revision.
    #[arg(long = "rev", value_name("NAME"), requires("compiletest"))]
    pub(crate) revisions: Vec<String>,
    /// Enable a Cargo-like feature.
    #[arg(short = 'f', long = "cargo-feature", value_name("NAME"))]
    pub(crate) cargo_features: Vec<String>,
    /// Enable an experimental rustc library or language feature.
    #[arg(short = 'F', long = "rustc-feature", value_name("NAME"))]
    pub(crate) rustc_features: Vec<String>,
    /// Output JSON instead of HTML.
    #[arg(short, long, conflicts_with("open"))]
    pub(crate) json: bool,
    /// Set the version of the (root) crate.
    #[arg(short = 'v', long, value_name("VERSION"))]
    pub(crate) crate_version: Option<String>,
    /// Document private items.
    #[arg(short = 'P', long)]
    pub(crate) private: bool,
    /// Document hidden items.
    #[arg(short = 'H', long)]
    pub(crate) hidden: bool,
    /// Document the memory layout of types.
    #[arg(long)]
    pub(crate) layout: bool,
    /// Generate links to definitions.
    #[arg(short = 'D', long)]
    pub(crate) link_to_definition: bool,
    /// Normalize types and constants.
    #[arg(long)]
    pub(crate) normalize: bool,
    /// Set the theme.
    #[arg(long, default_value("ayu"))]
    pub(crate) theme: String,
    /// Cap lints at a level.
    #[arg(long, value_name("LEVEL"), value_enum)]
    pub(crate) cap_lints: Option<LintLevel>,
    /// Enable rustc's `-Zverbose-internals`.
    #[arg(short = '#', long = "internals")]
    pub(crate) rustc_verbose_internals: bool,
    /// Override `RUSTC_LOG` to be `debug`.
    #[arg(long)]
    pub(crate) log: bool,
    /// Override `RUST_BACKTRACE` to be `0`.
    #[arg(short = 'B', long)]
    pub(crate) no_backtrace: bool,
}

/// Flags that are specific to `rrustdoc` itself.
#[derive(Parser)]
pub(crate) struct ProgramFlags {
    /// Use verbose output.
    #[arg(short = 'V', long)]
    pub(crate) verbose: bool,

    /// Run through without making any changes.
    #[arg(short = '0', long)]
    pub(crate) dry_run: bool,
}

impl ValueEnum for Edition {
    fn value_variants<'a>() -> &'a [Self] {
        Self::elements()
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.to_str()))
    }
}

#[derive(Clone)]
struct CrateNameParser;

impl TypedValueParser for CrateNameParser {
    type Value = CrateNameBuf;

    fn parse_ref(
        &self,
        command: &Command,
        _argument: Option<&Arg>,
        source: &OsStr,
    ) -> Result<Self::Value, Error> {
        let error = |kind| Error::new(kind).with_cmd(command);
        let source = source
            .to_str()
            .ok_or_else(|| error(ErrorKind::InvalidUtf8))?;

        CrateNameBuf::adjust_and_parse(source).map_err(|()| error(ErrorKind::InvalidValue))
    }
}

#[derive(Clone)]
struct CrateTypeParser;

impl TypedValueParser for CrateTypeParser {
    type Value = CrateType;

    fn parse_ref(
        &self,
        command: &Command,
        _argument: Option<&Arg>,
        source: &OsStr,
    ) -> Result<Self::Value, Error> {
        let error = |kind| Error::new(kind).with_cmd(command);
        let source = source
            .to_str()
            .ok_or_else(|| error(ErrorKind::InvalidUtf8))?;

        source.parse().map_err(|()| error(ErrorKind::InvalidValue))
    }

    // FIXME: possible values: bin, lib, rlib, proc-macro
}

impl ValueEnum for LintLevel {
    fn value_variants<'a>() -> &'a [Self] {
        Self::elements()
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.to_str()))
    }
}
