#![feature(let_chains, exit_status_error, type_alias_impl_trait, os_str_display, if_let_guard)]
#![deny(unused_must_use, rust_2018_idioms)]

use attribute::Attributes;
use builder::BuildMode;
use cli::InputPath;
use data::{CrateName, CrateNameBuf, CrateNameCow, CrateType, Edition};
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
    let (
        cli::Arguments {
            path,
            verbatim_flags,
            open,
            crate_name,
            crate_type,
            edition,
            build_flags,
            cross_crate,
            compiletest,
            program_flags,
            color,
        },
        toolchain,
    ) = cli::parse();

    match color {
        clap::ColorChoice::Always => owo_colors::set_override(true),
        clap::ColorChoice::Never => owo_colors::set_override(false),
        clap::ColorChoice::Auto => {}
    }

    // FIXME: Smh. move this into `cli::parse`.
    let path = cli::InputPath::parse(&path);

    // FIXME: eagerly lower `-f`s to `--cfg`s here, so we properly support them in `compiletest`+command

    // FIXME: Smh. move this into `cli::parse`.
    let build_mode = compute_build_mode(cross_crate, compiletest);

    let edition = edition.unwrap_or_else(|| match build_mode {
        BuildMode::Default | BuildMode::CrossCrate => Edition::LATEST_STABLE,
        BuildMode::Compiletest { .. } => Edition::default(),
    });

    let mut source = String::new();
    let (crate_name, crate_type) = compute_crate_name_and_type(
        crate_name,
        crate_type,
        build_mode,
        path,
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
        toolchain: toolchain.as_deref(),
        build: &build_flags,
        verbatim: verbatim_flags.as_ref(),
        program: &program_flags,
    };

    let crate_name =
        builder::build(build_mode, path, crate_name.as_ref(), crate_type, edition, flags)?;

    if open {
        command::open(crate_name.as_ref(), &program_flags)?;
    }

    Ok(())
}

fn compute_build_mode(cross_crate: bool, compiletest: bool) -> BuildMode {
    match (cross_crate, compiletest) {
        (true, false) => BuildMode::CrossCrate,
        (false, true) => BuildMode::Compiletest,
        (false, false) => BuildMode::Default,
        (true, true) => unreachable!(), // Already caught by `clap`.
    }
}

fn compute_crate_name_and_type<'src>(
    crate_name: Option<CrateNameBuf>,
    crate_type: Option<CrateType>,
    build_mode: BuildMode,
    path: InputPath<'_>,
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
                    *source = match path {
                        InputPath::Path(path) => std::fs::read_to_string(path)?,
                        InputPath::Stdin => todo!(), // FIXME
                    };
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
            let crate_name =
                crate_name.unwrap_or_else(|| CrateNameCow::parse_from_input_path(path).unwrap());
            let crate_type = crate_type.unwrap_or_default();

            (crate_name, crate_type)
        }
    })
}
