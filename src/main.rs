#![feature(decl_macro)]
#![feature(exact_size_is_empty)]
#![feature(exit_status_error)]
#![feature(impl_trait_in_assoc_type)]
#![feature(let_chains)]
#![feature(os_str_display)]
#![feature(substr_range)]
#![feature(type_alias_impl_trait)]
#![deny(rust_2018_idioms, unused_must_use, unused_crate_dependencies)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::if_not_else)] // I disagree
#![allow(clippy::items_after_statements)] // I disagree
#![allow(clippy::too_many_arguments)] // low priority
#![allow(clippy::too_many_lines)] // I disagree

use attribute::Attributes;
use data::{CrateNameBuf, CrateNameCow, CrateType, Edition};
use std::{path::Path, process::ExitCode};

mod attribute;
mod command;
mod data;
mod diagnostic;
mod directive;
mod error;
mod interface;
mod operate;
mod utility;

// FIXME: respect `compile-flags: --test`
// FIXME: Support for `-r`/`--release` maybe?

fn main() -> ExitCode {
    let result = try_main();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error.emit();
            ExitCode::FAILURE
        }
    }
}

fn try_main() -> error::Result {
    let args = interface::arguments();

    match args.color {
        clap::ColorChoice::Always => anstream::ColorChoice::Always.write_global(),
        clap::ColorChoice::Never => anstream::ColorChoice::Never.write_global(),
        clap::ColorChoice::Auto => {}
    }

    // FIXME: eagerly lower `-f`s to `--cfg`s here (or rather in `cli`?),
    // so we properly support them in `compiletest`+command

    // FIXME: this is awkward
    let edition = args.edition.unwrap_or_else(|| match args.command {
        interface::Command::Build { mode, .. } => mode.edition(),
        interface::Command::Doc { mode, .. } => mode.edition(),
    });

    let mut source = String::new();
    let (crate_name, crate_type) = compute_crate_name_and_type(
        args.crate_name,
        args.crate_type,
        // FIXME: this is awkward
        match args.command {
            interface::Command::Build { mode, .. } => {
                matches!(mode, operate::BuildMode::Compiletest)
            }
            interface::Command::Doc { mode, .. } => matches!(mode, operate::DocMode::Compiletest),
        },
        &args.path,
        edition,
        &args.build.cfgs,
        &args.debug,
        &mut source,
    )?;

    // FIXME: this is akward ... can we do this inside cli smh (not the ref op ofc)
    let verbatim_flags = command::VerbatimFlagsBuf {
        arguments: args.verbatim.iter().map(String::as_str).collect(),
        environment: Vec::new(),
    };
    // FIXME: this is akward ... can we do this inside cli smh (not the ref op ofc)
    let flags = command::Flags {
        toolchain: args.toolchain.as_deref(),
        build: &args.build,
        verbatim: verbatim_flags.as_ref(),
        debug: &args.debug,
    };

    match args.command {
        interface::Command::Build { run, mode } => {
            operate::build(mode, &args.path, crate_name.as_ref(), crate_type, edition, flags)?;

            if run {
                command::execute(Path::new(".").join(crate_name.as_str()), flags.debug)?;
            }
        }
        interface::Command::Doc { open, mode, flags: doc_flags } => {
            let crate_name = operate::document(
                mode,
                &args.path,
                crate_name.as_ref(),
                crate_type,
                edition,
                flags,
                &doc_flags,
            )?;

            if open {
                command::open(crate_name.as_ref(), &args.debug)?;
            }
        }
    }

    Ok(())
}

// FIXME: this is awkward
fn compute_crate_name_and_type<'src>(
    crate_name: Option<CrateNameBuf>,
    crate_type: Option<CrateType>,
    compiletest: bool,
    path: &Path,
    edition: Edition,
    cfgs: &[String],
    debug_flags: &interface::DebugFlags,
    source: &'src mut String,
) -> error::Result<(CrateNameCow<'src>, CrateType)> {
    Ok(match (crate_name, crate_type) {
        (Some(crate_name), Some(crate_type)) => (crate_name.into(), crate_type),
        (crate_name, crate_type) => {
            // FIXME: Not computing the crate name in compiletest mode is actually incorrect
            // since that leads to --open/--run failing to find the (correct) artifact.
            // So either use `-o` to force the location and get rid of the attr parsing code
            // or try to find it unconditionally.
            // NOTE: However, I don't want us to open the source file *twice* in compiletest
            // mode (once for attrs & once for directives). We should do it once if we go with
            // that approach
            let (crate_name, crate_type): (Option<CrateNameCow<'_>>, _) = if compiletest {
                (crate_name.map(Into::into), crate_type)
            } else {
                *source = std::fs::read_to_string(path)?; // FIXME: error context
                let attributes = Attributes::parse(
                    source,
                    // FIXME: doesn't contain `-f`s; eagerly expand them into `--cfg`s in main
                    cfgs,
                    edition,
                    debug_flags.verbose,
                );

                let crate_name: Option<CrateNameCow<'_>> =
                    crate_name.map(Into::into).or_else(|| attributes.crate_name.map(Into::into));

                (crate_name, crate_type.or(attributes.crate_type))
            };

            // FIXME: unwrap
            let crate_name = crate_name
                .unwrap_or_else(|| CrateNameBuf::adjust_and_parse_file_path(path).unwrap().into());
            let crate_type = crate_type.unwrap_or_default();

            (crate_name, crate_type)
        }
    })
}
