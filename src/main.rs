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
#![feature(trait_alias)]
#![feature(type_alias_impl_trait)]
#![feature(type_changing_struct_update)]
// Lints //
#![deny(rust_2018_idioms, unused_must_use, unused_crate_dependencies)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::if_not_else)] // I disagree
#![allow(clippy::items_after_statements)] // I disagree
#![allow(clippy::match_bool)] // I disagree
#![allow(clippy::too_many_arguments)] // low priority
#![allow(clippy::too_many_lines)] // I disagree

use data::{CrateNameBuf, CrateNameCow};
use diagnostic::{bug, fmt};
use std::process::ExitCode;

mod build;
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

    // FIXME: Keep the Option so we can do more stuff in operate
    let edition = args.edition.unwrap_or_else(|| args.operation.mode().edition());

    // FIXME: maybe delay this???
    // FIXME: This `unwrap` is obviously reachable (e.g., on `rrc '%$?'`)
    let crate_name: CrateNameCow<'_> = args.crate_name.map_or_else(
        || CrateNameBuf::adjust_and_parse_file_path(&args.path).unwrap().into(),
        Into::into,
    );
    // FIXME: Keep the option?
    let crate_type = args.crate_type.unwrap_or_default();

    // FIXME: this is awkward ... can we do this inside cli smh (not the ref op ofc)
    let verbatim = build::VerbatimDataBuf {
        arguments: args.verbatim.iter().map(String::as_str).collect(),
        variables: Vec::new(),
    };
    // FIXME: this is awkward ... can we do this inside cli smh (not the ref op ofc)
    let flags = build::Flags {
        toolchain: args.toolchain.as_deref(),
        build: &args.build,
        verbatim: verbatim.as_ref(),
        debug: &args.debug,
    };

    let crate_ =
        data::Crate { path: &args.path, name: crate_name.as_ref(), type_: crate_type, edition };

    operate::perform(args.operation, crate_, flags, cx)
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
