#![deny(unused_must_use, rust_2018_idioms)]
#![feature(
    let_chains,
    exit_status_error,
    type_alias_impl_trait,
    lazy_cell,
    byte_slice_trim_ascii,
    os_str_display,
    inline_const
)]

use attribute::Attributes;
use builder::{BuildMode, QueryMode};
use data::{CrateNameBuf, CrateNameCow, CrateType, Edition};
use diagnostic::IntoDiagnostic;
use std::{path::Path, process::ExitCode};

mod attribute;
mod builder;
mod cli;
mod command;
mod data;
mod diagnostic;
mod directive;
mod error;
mod parser;
mod utility;

// FIXME: respect `compile-flags: --test`
// FIXME: Add `--all-revs`.

fn main() -> ExitCode {
    let result = try_main();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error.into_diagnostic().emit();
            ExitCode::FAILURE
        }
    }
}

fn try_main() -> error::Result {
    let cli::Arguments {
        path,
        verbatim_flags,
        open,
        crate_name,
        crate_type,
        edition,
        build_flags,
        cross_crate,
        compiletest,
        query,
        program_flags,
        color,
    } = clap::Parser::parse();

    match color {
        clap::ColorChoice::Always => owo_colors::set_override(true),
        clap::ColorChoice::Never => owo_colors::set_override(false),
        clap::ColorChoice::Auto => {}
    }

    // FIXME: eagerly lower `-f`s to `--cfg`s here, so we properly support them in `compiletest`+command

    let query_mode = compute_query_mode(query, build_flags.json);
    let build_mode = compute_build_mode(cross_crate, compiletest, query_mode);

    let edition = edition.unwrap_or_else(|| match build_mode {
        BuildMode::Default | BuildMode::CrossCrate => Edition::LATEST_STABLE,
        BuildMode::Compiletest { .. } => Edition::default(),
    });

    let mut source = String::new();
    let (crate_name, crate_type) = compute_crate_name_and_type(
        crate_name,
        crate_type,
        build_mode,
        &path,
        edition,
        &build_flags.cfgs,
        &program_flags,
        &mut source,
    )?;

    let verbatim_flags = command::VerbatimFlagsBuf {
        arguments: verbatim_flags.iter().map(String::as_str).collect(),
        environment: Vec::new(),
    };
    let flags = command::Flags {
        build: &build_flags,
        verbatim: verbatim_flags.as_ref(),
        program: &program_flags,
    };

    let crate_name = builder::build(
        build_mode,
        &path,
        crate_name.as_ref(),
        crate_type,
        edition,
        flags,
    )?;

    if open {
        command::open(crate_name.as_ref(), &program_flags)?;
    }

    Ok(())
}

fn compute_query_mode(query: bool, json: bool) -> Option<QueryMode> {
    match (query, json) {
        (true, false) => Some(QueryMode::Html),
        (true, true) => Some(QueryMode::Json),
        (false, _) => None,
    }
}

fn compute_build_mode(cross_crate: bool, compiletest: bool, query: Option<QueryMode>) -> BuildMode {
    match (cross_crate, compiletest) {
        (true, false) => BuildMode::CrossCrate,
        (false, true) => BuildMode::Compiletest { query },
        (false, false) => BuildMode::Default,
        (true, true) => unreachable!(), // Already caught by `clap`.
    }
}

fn compute_crate_name_and_type<'src>(
    crate_name: Option<CrateNameBuf>,
    crate_type: Option<CrateType>,
    build_mode: BuildMode,
    path: &Path,
    edition: Edition,
    cfgs: &[String],
    program_flags: &cli::ProgramFlags,
    source: &'src mut String,
) -> error::Result<(CrateNameCow<'src>, CrateType)> {
    Ok(match (crate_name, crate_type) {
        (Some(crate_name), Some(crate_type)) => (crate_name.into(), crate_type),
        (crate_name, crate_type) => {
            let (crate_name, crate_type): (Option<CrateNameCow<'_>>, _) = match build_mode {
                BuildMode::Default | BuildMode::CrossCrate => {
                    *source = std::fs::read_to_string(path)?;
                    let attributes = Attributes::parse(
                        source,
                        // FIXME: doesn't contain `-f`s; eagerly expand them into `--cfg`s in main
                        cfgs,
                        edition,
                        program_flags.verbose,
                    );

                    let crate_name: Option<CrateNameCow<'_>> = crate_name
                        .map(Into::into)
                        .or_else(|| attributes.crate_name.map(Into::into));

                    (crate_name, crate_type.or(attributes.crate_type))
                }
                BuildMode::Compiletest { .. } => (crate_name.map(Into::into), crate_type),
            };

            // FIXME: unwrap
            let crate_name = crate_name.unwrap_or_else(|| {
                CrateNameBuf::adjust_and_parse_file_path(path)
                    .unwrap()
                    .into()
            });
            let crate_type = crate_type.unwrap_or_default();

            (crate_name, crate_type)
        }
    })
}
