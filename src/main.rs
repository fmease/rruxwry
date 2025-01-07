// Features //
#![feature(adt_const_params)]
#![feature(decl_macro)]
#![feature(exact_size_is_empty)]
#![feature(exit_status_error)]
#![feature(impl_trait_in_assoc_type)]
#![feature(iter_collect_into)]
#![feature(let_chains)]
#![feature(os_str_display)]
#![feature(substr_range)]
#![feature(type_alias_impl_trait)]
// Lints //
#![deny(rust_2018_idioms, unused_must_use, unused_crate_dependencies)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::if_not_else)] // I disagree
#![allow(clippy::items_after_statements)] // I disagree
#![allow(clippy::too_many_arguments)] // low priority
#![allow(clippy::too_many_lines)] // I disagree

use attribute::Attributes;
use data::{CrateNameBuf, CrateNameCow, CrateType, Edition};
use diagnostic::{bug, fmt};
use operate::Mode;
use std::{path::Path, process::ExitCode};

mod attribute;
mod command;
mod context;
mod data;
mod diagnostic;
mod directive;
mod error;
mod interface;
mod operate;
mod source;
mod utility;

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
    set_panic_hook();

    let args = interface::arguments();

    match args.color {
        clap::ColorChoice::Always => anstream::ColorChoice::Always.write_global(),
        clap::ColorChoice::Never => anstream::ColorChoice::Never.write_global(),
        clap::ColorChoice::Auto => {}
    }

    // FIXME: eagerly lower `-f`s to `--cfg`s here (or rather in `cli`?),
    // so we properly support them in `compiletest`+command

    let mode = args.command.mode();
    let edition = args.edition.unwrap_or_else(|| mode.edition());

    let mut source = String::new();
    let (crate_name, crate_type) = compute_crate_name_and_type(
        args.crate_name,
        args.crate_type,
        mode,
        &args.path,
        edition,
        &args.build.cfgs,
        &args.debug,
        &mut source,
    )?;

    // FIXME: this is awkward ... can we do this inside cli smh (not the ref op ofc)
    let verbatim_flags = command::VerbatimFlagsBuf {
        arguments: args.verbatim.iter().map(String::as_str).collect(),
        environment: Vec::new(),
    };
    // FIXME: this is awkward ... can we do this inside cli smh (not the ref op ofc)
    let flags = command::Flags {
        toolchain: args.toolchain.as_deref(),
        build: &args.build,
        verbatim: verbatim_flags.as_ref(),
        debug: &args.debug,
    };

    let crate_ =
        operate::Crate { path: &args.path, name: crate_name.as_ref(), type_: crate_type, edition };

    let cx = context::ContextData::default();
    let cx = context::Context::new(&cx);

    match args.command {
        interface::Command::Build { run, mode } => {
            operate::build(mode, crate_, flags, cx)?;

            if run {
                command::execute(Path::new(".").join(crate_name.as_str()), flags.debug)?;
            }
        }
        interface::Command::Doc { open, mode, flags: doc_flags } => {
            let crate_name = operate::document(mode, crate_, flags, &doc_flags, cx)?;

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
    mode: Mode,
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
            let (crate_name, crate_type): (Option<CrateNameCow<'_>>, _) = match mode {
                Mode::Compiletest => (crate_name.map(Into::into), crate_type),
                Mode::Other => {
                    *source = std::fs::read_to_string(path)?; // FIXME: error context
                    let attributes = Attributes::parse(
                        source,
                        // FIXME: doesn't contain `-f`s; eagerly expand them into `--cfg`s in main
                        cfgs,
                        edition,
                        debug_flags.verbose,
                    );

                    let crate_name: Option<CrateNameCow<'_>> = crate_name
                        .map(Into::into)
                        .or_else(|| attributes.crate_name.map(Into::into));

                    (crate_name, crate_type.or(attributes.crate_type))
                }
            };

            // FIXME: unwrap
            let crate_name = crate_name
                .unwrap_or_else(|| CrateNameBuf::adjust_and_parse_file_path(path).unwrap().into());
            let crate_type = crate_type.unwrap_or_default();

            (crate_name, crate_type)
        }
    })
}

fn set_panic_hook() {
    const ENV_VAR: &str = "RRUXWRY_BACKTRACE";

    std::panic::set_hook(Box::new(|information| {
        let payload = information.payload();

        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<unknown cause>");

        let backtrace = std::env::var(ENV_VAR)
            .is_ok_and(|variable| variable != "0")
            .then(std::backtrace::Backtrace::force_capture);

        let error = bug(fmt!("{message}"));
        let error = match information.location() {
            Some(location) => error.note(fmt!("at `{location}`")),
            None => error,
        };
        let error = match std::thread::current().name() {
            Some(name) => error.note(fmt!("in thread `{name}`")),
            None => error.note(fmt!("in an unknown thread")),
        };
        let error = error.note(fmt!(
            "rruxwry unexpectedly panicked. this is a bug. we would appreciate a bug report"
        ));
        let error = match backtrace {
            Some(backtrace) => error.note(fmt!("with the following backtrace:\n{backtrace}")),
            None => error
                .note(fmt!("rerun with environment variable `{ENV_VAR}=1` to display a backtrace")),
        };
        error.finish();
    }));
}
