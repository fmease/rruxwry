// Features //
#![feature(adt_const_params)]
#![feature(ascii_char_variants)]
#![feature(ascii_char)]
#![feature(decl_macro)]
#![feature(exact_size_is_empty)]
#![feature(exit_status_error)]
// FIXME: Switch to feature `min_generic_const_args` as soon as it no longer ICEs with:
//            thread 'rustc' panicked at compiler/rustc_trait_selection/src/traits/normalize.rs:465:21:
//            internal error: entered unreachable code: unexpected `DefKind` for const alias to resolve to: Ctor(Variant, Const)
//        Tracked in: <https://github.com/rust-lang/rust/issues/141738>.
#![feature(generic_const_exprs)]
// FIXME: Only used for a GCE workaround. Remove once we use mGCA.
#![feature(generic_const_items)]
#![feature(if_let_guard)]
#![feature(iter_collect_into)]
#![feature(slice_split_once)]
#![feature(str_split_remainder)]
#![feature(substr_range)]
#![feature(super_let)]
#![feature(trait_alias)]
#![feature(type_changing_struct_update)]
// Lints //
#![expect(incomplete_features, reason = "generic_const_exprs")]
#![deny(rust_2018_idioms, unused_must_use, unused_crate_dependencies)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::if_not_else)] // I disagree
#![allow(clippy::items_after_statements)] // I disagree
#![allow(clippy::match_bool)] // I disagree
#![allow(clippy::option_option)] // I disagree
#![allow(clippy::too_many_lines)] // I disagree

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

    let krate = data::Crate {
        path: args.path.as_deref(),
        name: args.crate_name.as_ref().map(|name| name.as_ref()),
        typ: args.crate_type,
        edition: args.edition,
    };

    let opts = build::Options {
        b_opts: args.b_opts,
        v_opts: build::VerbatimOptions {
            arguments: args.verbatim.iter().map(String::as_str).collect(),
            variables: Vec::new(),
        },
    };

    let cx = context::new!(context::Options { toolchain: args.toolchain, dbg_opts: args.dbg_opts });

    operate::perform(args.operation, krate, opts, cx)
}

fn set_panic_hook() {
    const ENV_VAR: &str = "RRUXWRY_BACKTRACE";

    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();

        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<unknown cause>");

        let backtrace = std::env::var(ENV_VAR)
            .is_ok_and(|variable| variable != "0")
            .then(std::backtrace::Backtrace::force_capture);

        let diag = bug(fmt!("{message}"));
        let diag = match info.location() {
            Some(location) => diag.note(fmt!("at `{location}`")),
            None => diag,
        };
        let diag = match std::thread::current().name() {
            Some(name) => diag.note(fmt!("in thread `{name}`")),
            None => diag.note(fmt!("in an unknown thread")),
        };
        let diag = diag.note(fmt!(
            "rruxwry unexpectedly panicked. this is a bug. we would appreciate a bug report"
        ));
        let diag = match backtrace {
            Some(backtrace) => diag.note(fmt!("with the following backtrace:\n{backtrace}")),
            None => diag
                .note(fmt!("rerun with environment variable `{ENV_VAR}=1` to display a backtrace")),
        };
        diag.done();
    }));
}
