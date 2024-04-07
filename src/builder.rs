//! High-level build commands.
//!
//! The low-level build commands are defined in [`crate::command`].

use crate::{
    command::{
        self, CrateName, CrateNameCow, CrateNameRef, CrateType, Edition, ExternCrate, Flags,
        Strictness,
    },
    diagnostic::{error, Diagnostic, IntoDiagnostic},
    directive::Directives,
    error::Result,
    utility::default,
};
use joinery::JoinableIterator;
use rustc_hash::FxHashSet;
use std::{borrow::Cow, cell::LazyCell, mem, path::Path};

pub(crate) fn build<'a>(
    mode: BuildMode,
    path: &Path,
    crate_name: CrateNameRef<'a>,
    crate_type: CrateType,
    edition: Edition,
    flags: Flags<'_>,
) -> Result<CrateNameCow<'a>> {
    match mode {
        BuildMode::Default => build_default(path, crate_name, crate_type, edition, flags),
        BuildMode::CrossCrate => build_cross_crate(path, crate_name, crate_type, edition, flags),
        BuildMode::Compiletest { query } => {
            build_compiletest(path, crate_name, edition, flags, query)
        }
    }
}

fn build_default<'a>(
    path: &Path,
    crate_name: CrateNameRef<'a>,
    crate_type: CrateType,
    edition: Edition,
    flags: Flags<'_>,
) -> Result<CrateNameCow<'a>> {
    command::document(
        path,
        crate_name,
        crate_type,
        edition,
        crate_type.crates(),
        flags,
        Strictness::Lenient,
    )?;

    Ok(crate_name.map(Cow::Borrowed))
}

fn build_cross_crate(
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    flags: Flags<'_>,
) -> Result<CrateNameCow<'static>> {
    command::compile(
        path,
        crate_name,
        crate_type.to_non_executable(),
        edition,
        crate_type.crates(),
        flags,
        Strictness::Lenient,
    )?;

    let dependent_crate_name = CrateName::new(format!("u_{crate_name}"));
    let dependent_crate_path = path
        .with_file_name(dependent_crate_name.as_str())
        .with_extension("rs");

    if !flags.program.dry_run && !dependent_crate_path.exists() {
        // While we could omit the `extern crate` declaration in `edition >= Edition::Edition2018`,
        // we would need to recreate the file on each rerun if the edition was 2015 instead of
        // skipping that step since we wouldn't know whether the existing file if applicable was
        // created for a newer edition or not.
        std::fs::write(
            &dependent_crate_path,
            format!("extern crate {crate_name}; pub use {crate_name}::*;\n"),
        )?;
    };

    command::document(
        &dependent_crate_path,
        dependent_crate_name.as_ref(),
        default(),
        edition,
        &[ExternCrate::Named {
            name: crate_name.as_ref(),
            path: None,
        }],
        flags,
        Strictness::Lenient,
    )?;

    Ok(dependent_crate_name.map(Cow::Owned))
}

fn build_compiletest<'a>(
    path: &Path,
    crate_name: CrateNameRef<'a>,
    _edition: Edition, // FIXME: should we respect the edition or should we reject it with `clap`?
    flags: Flags<'_>,
    query: Option<QueryMode>,
) -> Result<CrateNameCow<'a>> {
    // FIXME: Add a flag `--all-revs`.
    // FIXME: Make sure `//@ compile-flags: --extern name` works as expected
    let source = std::fs::read_to_string(path)?;
    let directives = Directives::parse(&source, query);

    // FIXME: We should also store Cargo-like features here after having converted them to
    // cfg specs NOTE: This will be fixed once we eagerly expand `-f` to `--cfg`.
    // FIXME: Is it actually possible to write `//[feature="name"]@` and have compiletest understand it?
    let mut revisions = FxHashSet::default();

    for revision in &flags.build.revisions {
        if !directives.revisions.contains(revision.as_str()) {
            let error = Error::UnknownRevision {
                unknown: revision.clone(),
                available: directives
                    .revisions
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            };
            return Err(error.into());
        }

        revisions.insert(revision.as_str());
    }

    revisions.extend(flags.build.cfgs.iter().map(String::as_str));

    let mut directives = directives.into_instantiated(&revisions);

    // FIXME: unwrap
    let auxiliary_base_path = LazyCell::new(|| path.parent().unwrap().join("auxiliary"));

    let dependencies: Vec<_> = directives
        .dependencies
        .iter()
        .map(|dependency| {
            build_compiletest_auxiliary(
                dependency,
                &auxiliary_base_path,
                directives.build_aux_docs,
                flags,
            )
        })
        .collect::<Result<_>>()?;

    let verbatim_flags = mem::take(&mut directives.verbatim_flags).extended(flags.verbatim);
    let flags = Flags {
        verbatim: verbatim_flags.as_ref(),
        ..flags
    };

    command::document(
        path,
        crate_name,
        default(), // FIXME: respect `@compile-flags: --crate-type`
        directives.edition.unwrap_or_default(),
        &dependencies,
        flags,
        Strictness::Strict,
    )?;

    Ok(crate_name.map(Cow::Borrowed))
}

// FIXME: Support nested auxiliaries!
fn build_compiletest_auxiliary<'a>(
    extern_crate: &ExternCrate<'a>,
    base_path: &Path,
    document: bool,
    flags: Flags<'_>,
) -> Result<ExternCrate<'a>> {
    let path = match extern_crate {
        ExternCrate::Unnamed { path } => base_path.join(path),
        ExternCrate::Named { name, path } => match path {
            Some(path) => base_path.join(path.as_ref()),
            None => base_path.join(name.as_str()).with_extension("rs"),
        },
    };

    let source = std::fs::read_to_string(&path);

    // FIXME: unwrap
    let crate_name = CrateName::from_path(&path).unwrap();

    // FIXME: What about instantiation???
    let mut directives = source
        .as_ref()
        .map(|source| Directives::parse(source, None))
        .unwrap_or_default();

    let edition = directives.edition.unwrap_or_default();

    let verbatim_flags = mem::take(&mut directives.verbatim_flags).extended(flags.verbatim);
    let flags = Flags {
        verbatim: verbatim_flags.as_ref(),
        ..flags
    };

    command::compile(
        &path,
        crate_name.as_ref(),
        // FIXME: Verify this works with `@compile-flags:--crate-type=proc-macro`
        // FIXME: I don't think it works rn
        CrateType::Lib,
        edition,
        &[],
        flags,
        Strictness::Strict,
    )?;

    // FIXME: Is this how `//@ build-aux-docs` is supposed to work?
    if document {
        command::document(
            &path,
            crate_name.as_ref(),
            // FIXME: Verify this works with `@compile-flags:--crate-type=proc_macro`
            // FIXME: I don't think it works rn
            default(),
            edition,
            &[],
            flags,
            Strictness::Strict,
        )?;
    }

    // FIXME: Do we need to respect `compile-flags: --crate-name` and adjust `ExternCrate` accordingly?
    Ok(match *extern_crate {
        // FIXME: probably doesn't handle `//@ aux-build: ../file.rs` correctly since `-L.` wouldn't pick it up
        ExternCrate::Unnamed { path } => ExternCrate::Unnamed { path },
        // FIXME: For some reason `compiletest` doesn't support `//@ aux-crate: name=../`
        ExternCrate::Named { name, .. } => {
            // FIXME: unwrap
            let crate_name = CrateName::from_path(&path).unwrap();

            ExternCrate::Named {
                name,
                // FIXME: needs to be relative to the base_path
                // FIXME: layer violation?? should this be the job of mod command?
                path: (name != crate_name.as_ref()).then(|| format!("lib{crate_name}.rlib").into()),
            }
        }
    })
}

#[derive(Clone, Copy)]
pub(crate) enum BuildMode {
    Default,
    CrossCrate,
    Compiletest { query: Option<QueryMode> },
}

#[derive(Clone, Copy)]
pub(crate) enum QueryMode {
    Html,
    Json,
}

pub(crate) enum Error {
    UnknownRevision {
        unknown: String,
        available: FxHashSet<String>,
    },
}

impl IntoDiagnostic for Error {
    fn into_diagnostic(self) -> Diagnostic {
        match self {
            Error::UnknownRevision { unknown, available } => {
                let available = available
                    .iter()
                    .map(|revision| format!("`{revision}`"))
                    .join_with(", ");

                error(format!("unknown revision `{unknown}`"))
                    .note(format!("available revisions are: {available}"))
                    .note(format!(
                        "you can use `--cfg` over `--rev` to suppress this check"
                    ))
            }
        }
    }
}
