#![deny(unused_must_use, rust_2018_idioms)]
#![feature(
    let_chains,
    exit_status_error,
    type_alias_impl_trait,
    lazy_cell,
    byte_slice_trim_ascii
)]

use attribute::Attributes;
use builder::{BuildMode, QueryMode};
use command::{CrateNameBuf, CrateNameCow, Edition};

mod attribute;
mod builder;
mod cli;
mod command;
mod directive;
mod error;
mod parser;
mod utility;

// FIXME: respect `compile-flags: --test`
// FIXME: Support passing additional arguments verbatim to `rustc` & `rustdoc`
//        via `RUSTFLAGS`/`RUSTDOCFLAGS`.
// FIXME: Should we add `--rev` for `--ui-test` that's basically `--cfg` but checked against `//@ revisions`?
//        Or should we just warn if a cfg key is similar to a revision name?
// FIXME: Add `--all-revs`.

fn main() -> error::Result {
    let cli::Arguments {
        path,
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

    let build_mode = match (cross_crate, compiletest) {
        (true, false) => BuildMode::CrossCrate,
        (false, true) => BuildMode::Compiletest {
            query: match (query, build_flags.json) {
                (true, false) => Some(QueryMode::Html),
                (true, true) => Some(QueryMode::Json),
                (false, _) => None,
            },
        },
        (false, false) => BuildMode::Default,
        (true, true) => unreachable!(), // Already caught by `clap`.
    };

    let edition = edition.unwrap_or_else(|| match build_mode {
        BuildMode::Default | BuildMode::CrossCrate => Edition::LATEST_STABLE,
        BuildMode::Compiletest { .. } => Edition::default(),
    });

    let source;
    let (crate_name, crate_type) = match (crate_name, crate_type) {
        (Some(crate_name), Some(crate_type)) => (crate_name.into(), crate_type),
        (crate_name, crate_type) => {
            let (crate_name, crate_type): (Option<CrateNameCow<'_>>, _) = match build_mode {
                BuildMode::Default | BuildMode::CrossCrate => {
                    source = std::fs::read_to_string(&path)?;
                    // FIXME: doesn't contain `-f`s; eagerly expand them into `--cfg`s above
                    let attributes = Attributes::parse(
                        &source,
                        &build_flags.cfgs,
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
                crate_name.unwrap_or_else(|| CrateNameBuf::from_path(&path).unwrap().into());
            let crate_type = crate_type.unwrap_or_default();

            (crate_name, crate_type)
        }
    };

    let crate_name = builder::build(
        build_mode,
        &path,
        crate_name.as_ref(),
        crate_type,
        edition,
        &build_flags,
        &program_flags,
    )?;

    if open {
        command::open(crate_name.as_ref(), &program_flags)?;
    }

    Ok(())
}
