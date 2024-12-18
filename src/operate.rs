//! High-level build operations.
//!
//! The low-level build commands are defined in [`crate::command`].

use crate::{
    command::{self, ExternCrate, Flags, Strictness},
    data::{CrateName, CrateNameCow, CrateNameRef, CrateType, DocBackend, Edition},
    diagnostic::{Diagnostic, IntoDiagnostic, error},
    directive,
    error::Result,
    utility::default,
};
use joinery::JoinableIterator;
use std::{borrow::Cow, cell::LazyCell, collections::BTreeSet, mem, path::Path};

pub(crate) fn build(
    mode: BuildMode,
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    flags: Flags<'_>,
) -> Result {
    match mode {
        BuildMode::Default => build_default(path, crate_name, crate_type, edition, flags),
        BuildMode::Compiletest => build_compiletest(path, crate_name, crate_type, edition, flags),
    }
}

fn build_default(
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    flags: Flags<'_>,
) -> Result {
    command::compile(
        path,
        crate_name,
        crate_type,
        edition,
        extern_prelude_for(crate_type),
        flags,
        Strictness::Lenient,
    )
}

fn build_compiletest(
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    // FIXME: use it
    _edition: Edition,
    flags: Flags<'_>,
) -> Result {
    // FIXME: Add a flag `--all-revs`.
    // FIXME: Make sure `//@ compile-flags: --extern name` works as expected
    let source = std::fs::read_to_string(path)?; // FIXME: error context
    let directives = directive::parse(&source, directive::Scope::Base);

    // FIXME: We should also store Cargo-like features here after having converted them to
    // cfg specs NOTE: This will be fixed once we eagerly expand `-f` to `--cfg`.
    // FIXME: Is it actually possible to write `//[feature="name"]@` and have compiletest understand it?
    let mut revisions = BTreeSet::default();

    for revision in &flags.build.revisions {
        if !directives.revisions.contains(revision.as_str()) {
            let error = Error::UnknownRevision {
                unknown: revision.clone(),
                available: directives.revisions.iter().map(ToString::to_string).collect(),
            };
            return Err(error.into());
        }

        revisions.insert(revision.as_str());
    }

    revisions.extend(flags.build.cfgs.iter().map(String::as_str));

    let mut directives = directives.into_instantiated(&revisions);

    let verbatim_flags = mem::take(&mut directives.verbatim_flags).extended(flags.verbatim);
    let flags = Flags { verbatim: verbatim_flags.as_ref(), ..flags };

    command::compile(
        path,
        crate_name,
        // FIXME: Once we support `//@ proc-macro` we need to honor the implicit crate_type==Lib (of the host) here.
        crate_type,
        directives.edition.unwrap_or(Edition::RUSTC_DEFAULT),
        // FIXME: Respect `//@ aux-crate` etc.
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client) similar to `extern_prelude_for` here.
        &[],
        flags,
        Strictness::Strict,
    )
}

pub(crate) fn document<'a>(
    mode: DocMode,
    path: &Path,
    crate_name: CrateNameRef<'a>,
    crate_type: CrateType,
    edition: Edition,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &crate::interface::DocFlags,
) -> Result<CrateNameCow<'a>> {
    match mode {
        DocMode::Default => {
            document_default(path, crate_name, crate_type, edition, flags, doc_flags)
        }
        DocMode::CrossCrate => {
            document_cross_crate(path, crate_name, crate_type, edition, flags, doc_flags)
        }
        DocMode::Compiletest => {
            document_compiletest(path, crate_name, crate_type, edition, flags, doc_flags)
        }
    }
}

fn document_default<'a>(
    path: &Path,
    crate_name: CrateNameRef<'a>,
    crate_type: CrateType,
    edition: Edition,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &crate::interface::DocFlags,
) -> Result<CrateNameCow<'a>> {
    command::document(
        path,
        crate_name,
        crate_type,
        edition,
        extern_prelude_for(crate_type),
        flags,
        doc_flags,
        Strictness::Lenient,
    )?;

    Ok(crate_name.map(Cow::Borrowed))
}

fn document_cross_crate(
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &crate::interface::DocFlags,
) -> Result<CrateNameCow<'static>> {
    command::compile(
        path,
        crate_name,
        crate_type.to_non_executable(),
        edition,
        extern_prelude_for(crate_type),
        flags,
        Strictness::Lenient,
    )?;

    let dependent_crate_name = CrateName::new_unchecked(format!("u_{crate_name}"));
    let dependent_crate_path =
        path.with_file_name(dependent_crate_name.as_str()).with_extension("rs");

    if !flags.debug.dry_run && !dependent_crate_path.exists() {
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
        &[ExternCrate::Named { name: crate_name.as_ref(), path: None }],
        flags,
        doc_flags,
        Strictness::Lenient,
    )?;

    Ok(dependent_crate_name.map(Cow::Owned))
}

fn extern_prelude_for(crate_type: CrateType) -> &'static [ExternCrate<'static>] {
    match crate_type {
        // For convenience and just like Cargo we add `libproc_macro` to the external prelude.
        CrateType::ProcMacro => &[ExternCrate::Named {
            name: const { CrateName::new_unchecked("proc_macro") },
            path: None,
        }],
        _ => [].as_slice(),
    }
}

fn document_compiletest<'a>(
    path: &Path,
    crate_name: CrateNameRef<'a>,
    crate_type: CrateType,
    // FIXME: respect the CLI edition, it should override `//@edition`
    //        yes, it would lead to a failure  for e.g. `//@compile-args:--edition`
    _edition: Edition,
    flags: Flags<'_>,
    // FIXME: tempory
    doc_flags: &crate::interface::DocFlags,
) -> Result<CrateNameCow<'a>> {
    // FIXME: Add a flag `--all-revs`.
    // FIXME: Make sure `//@ compile-flags: --extern name` works as expected
    let source = std::fs::read_to_string(path)?; // FIXME: error context
    let scope = match doc_flags.backend {
        DocBackend::Html => directive::Scope::HtmlDocCk,
        DocBackend::Json => directive::Scope::JsonDocCk,
    };
    let directives = directive::parse(&source, scope);

    // FIXME: We should also store Cargo-like features here after having converted them to
    // cfg specs NOTE: This will be fixed once we eagerly expand `-f` to `--cfg`.
    // FIXME: Is it actually possible to write `//[feature="name"]@` and have compiletest understand it?
    let mut revisions = BTreeSet::default();

    for revision in &flags.build.revisions {
        if !directives.revisions.contains(revision.as_str()) {
            let error = Error::UnknownRevision {
                unknown: revision.clone(),
                available: directives.revisions.iter().map(ToString::to_string).collect(),
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
            document_compiletest_auxiliary(
                dependency,
                &auxiliary_base_path,
                directives.build_aux_docs,
                flags,
                doc_flags,
            )
        })
        .collect::<Result<_>>()?;

    let verbatim_flags = mem::take(&mut directives.verbatim_flags).extended(flags.verbatim);
    let flags = Flags { verbatim: verbatim_flags.as_ref(), ..flags };

    command::document(
        path,
        crate_name,
        // FIXME: Once we support `//@ proc-macro` we need to honor the implicit crate_type==Lib (of the host) here.
        crate_type,
        directives.edition.unwrap_or(Edition::RUSTC_DEFAULT),
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client) similar to `extern_prelude_for` here.
        &dependencies,
        flags,
        doc_flags,
        Strictness::Strict,
    )?;

    Ok(crate_name.map(Cow::Borrowed))
}

// FIXME: Support nested auxiliaries!
fn document_compiletest_auxiliary<'a>(
    extern_crate: &ExternCrate<'a>,
    base_path: &Path,
    document: bool,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &crate::interface::DocFlags,
) -> Result<ExternCrate<'a>> {
    let path = match extern_crate {
        ExternCrate::Unnamed { path } => base_path.join(path),
        ExternCrate::Named { name, path } => match path {
            Some(path) => base_path.join(path.as_ref()),
            None => base_path.join(name.as_str()).with_extension("rs"),
        },
    };

    let source = std::fs::read_to_string(&path); // FIXME: error context

    // FIXME: unwrap
    let crate_name = CrateName::adjust_and_parse_file_path(&path).unwrap();

    // FIXME: DRY
    let scope = match doc_flags.backend {
        DocBackend::Html => directive::Scope::HtmlDocCk,
        DocBackend::Json => directive::Scope::JsonDocCk,
    };

    // FIXME: What about instantiation???
    let mut directives =
        source.as_ref().map(|source| directive::parse(source, scope)).unwrap_or_default();

    let edition = directives.edition.unwrap_or(Edition::RUSTC_DEFAULT);

    let verbatim_flags = mem::take(&mut directives.verbatim_flags).extended(flags.verbatim);
    let flags = Flags { verbatim: verbatim_flags.as_ref(), ..flags };

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
            doc_flags,
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
            let crate_name = CrateName::adjust_and_parse_file_path(&path).unwrap();

            ExternCrate::Named {
                name,
                // FIXME: needs to be relative to the base_path
                // FIXME: layer violation?? should this be the job of mod command?
                path: (name != crate_name.as_ref()).then(|| format!("lib{crate_name}.rlib").into()),
            }
        }
    })
}

// FIXME: Is there are way to consolidate DocMode and BuildMode?
//        Plz DRY the compiletest edition code.

#[derive(Clone, Copy)]
pub(crate) enum DocMode {
    Default,
    CrossCrate,
    Compiletest,
}

impl DocMode {
    pub(crate) fn edition(self) -> Edition {
        match self {
            Self::Default | Self::CrossCrate => Edition::LATEST_STABLE,
            Self::Compiletest => Edition::RUSTC_DEFAULT,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BuildMode {
    Default,
    Compiletest,
}

impl BuildMode {
    pub(crate) fn edition(self) -> Edition {
        match self {
            Self::Default => Edition::LATEST_STABLE,
            Self::Compiletest => Edition::RUSTC_DEFAULT,
        }
    }
}

pub(crate) enum Error {
    UnknownRevision { unknown: String, available: BTreeSet<String> },
}

impl IntoDiagnostic for Error {
    fn into_diagnostic(self) -> Diagnostic {
        match self {
            Error::UnknownRevision { unknown, available } => {
                let available =
                    available.iter().map(|revision| format!("`{revision}`")).join_with(", ");

                error(format!("unknown revision `{unknown}`"))
                    .note(format!("available revisions are: {available}"))
                    .note("you can use `--cfg` over `--rev` to suppress this check")
            }
        }
    }
}
