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

use attribute::Attrs;
use context::Context;
use data::{CrateNameBuf, CrateNameCow, CrateType, Edition};
use diagnostic::{bug, error, fmt};
use source::Spanned;
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

    context::initialize!(cx);

    // FIXME: eagerly lower `-f`s to `--cfg`s here (or rather in `cli`?),
    // so we properly support them in `compiletest`+command

    let edition = args.edition.unwrap_or_else(|| args.command.mode().edition());

    let (crate_name, crate_type) = locate_crate_name_and_type(
        args.crate_name,
        args.crate_type,
        &args.path,
        edition,
        &args.debug,
        cx,
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

    match args.command {
        interface::Command::Build { run, mode } => {
            operate::build(mode, crate_, flags, cx)?;

            if run {
                command::execute(Path::new(".").join(crate_name.as_str()), flags.debug).map_err(
                    |error| {
                        self::error(fmt!("failed to run the built binary"))
                            .note(fmt!("{error}"))
                            .finish()
                    },
                )?;
            }
        }
        interface::Command::Doc { open, mode, flags: doc_flags } => {
            let crate_name = operate::document(mode, crate_, flags, &doc_flags, cx)?;

            if open {
                command::open(crate_name.as_ref(), &args.debug).map_err(|error| {
                    self::error(fmt!("failed to open the generated docs in a browser"))
                        .note(fmt!("{error}"))
                        .finish()
                })?;
            }
        }
    }

    Ok(())
}

// FIXME: this is awkward
fn locate_crate_name_and_type<'cx>(
    crate_name: Option<CrateNameBuf>,
    crate_type: Option<CrateType>,
    path: &Path,
    edition: Edition,
    debug_flags: &interface::DebugFlags,
    cx: Context<'cx>,
) -> crate::error::Result<(CrateNameCow<'cx>, CrateType)> {
    Ok(match (crate_name, crate_type) {
        (Some(crate_name), Some(crate_type)) => (crate_name.into(), crate_type),
        (crate_name, crate_type) => {
            let source = cx.map().add(Spanned::sham(path), cx)?.contents;
            let attrs = Attrs::parse(source, edition, debug_flags.verbose);

            // FIXME: unwrap
            let crate_name = crate_name
                .map(Into::into)
                .or_else(|| attrs.crate_name.map(Into::into))
                .unwrap_or_else(|| CrateNameBuf::adjust_and_parse_file_path(path).unwrap().into());
            let crate_type = crate_type.or(attrs.crate_type).unwrap_or_default();

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

        let it = bug(fmt!("{message}"));
        let it = match information.location() {
            Some(location) => it.note(fmt!("at `{location}`")),
            None => it,
        };
        let it = match std::thread::current().name() {
            Some(name) => it.note(fmt!("in thread `{name}`")),
            None => it.note(fmt!("in an unknown thread")),
        };
        let it = it.note(fmt!(
            "rruxwry unexpectedly panicked. this is a bug. we would appreciate a bug report"
        ));
        let it = match backtrace {
            Some(backtrace) => it.note(fmt!("with the following backtrace:\n{backtrace}")),
            None => it
                .note(fmt!("rerun with environment variable `{ENV_VAR}=1` to display a backtrace")),
        };
        it.finish();
    }));
}
