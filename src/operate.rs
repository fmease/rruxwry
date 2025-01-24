//! High-level build operations.
//!
//! The low-level build commands are defined in [`crate::command`].

// FIXME: Create test for `//@ compile-flags: --extern name` + aux-build
// FIXME: Create test for `//@ compile-flags: --test`.

use crate::{
    command::{self, Edition, ExternCrate, Flags, Strictness},
    context::Context,
    data::{self, CrateName, CrateNameCow, CrateNameRef, CrateType, DocBackend},
    directive,
    error::Result,
    source::Spanned,
    utility::default,
};
use std::{borrow::Cow, cell::LazyCell, mem, path::Path};

#[derive(Clone, Copy)]
pub(crate) struct Crate<'a> {
    pub(crate) path: &'a Path,
    pub(crate) name: CrateNameRef<'a>,
    pub(crate) type_: CrateType,
    pub(crate) edition: data::Edition,
}

pub(crate) fn build(
    mode: BuildMode,
    crate_: Crate<'_>,
    flags: Flags<'_>,
    cx: Context<'_>,
) -> Result {
    match mode {
        BuildMode::Default => build_default(crate_, flags),
        BuildMode::Compiletest(flavor) => build_compiletest(crate_, flags, flavor, cx),
    }
}

fn build_default(crate_: Crate<'_>, flags: Flags<'_>) -> Result {
    command::compile(
        crate_.path,
        crate_.name,
        crate_.type_,
        Some(Edition::Parsed(crate_.edition)),
        extern_prelude_for(crate_.type_),
        flags,
        Strictness::Lenient,
    )
    .map_err(Into::into)
}

fn build_compiletest(
    crate_: Crate<'_>,
    flags: Flags<'_>,
    flavor: directive::Flavor,
    cx: Context<'_>,
) -> Result {
    let mut directives = directive::gather(
        Spanned::sham(crate_.path),
        directive::Scope::Base,
        directive::Role::Principal,
        flavor,
        flags.build.revision.as_deref(),
        cx,
    )?;

    // FIXME: unwrap
    let aux_base_path = LazyCell::new(|| crate_.path.parent().unwrap().join("auxiliary"));

    let dependencies: Vec<_> = directives
        .dependencies
        .iter()
        .map(|dep| build_compiletest_auxiliary(dep, &aux_base_path, flags, flavor, cx))
        .collect::<Result<_>>()?;

    let verbatim = mem::take(&mut directives.verbatim).extended(flags.verbatim);
    let flags = Flags { verbatim: verbatim.as_ref(), ..flags };

    // FIXME: Should we generally emit a warning when CLI tests anything that *may* lead to clashes down the line?
    //        E.g., on `--crate-name`, `--crate-type` and `--edition`? And what about CLI/env verbatim flags?

    // FIXME: Once this one is an `Option<Edition>` instead, so we can tell if it was explicitly set by the user,
    //        use it to overwrite `directives.edition`.
    let _ = crate_.edition;

    command::compile(
        crate_.path,
        crate_.name,
        // FIXME: Once we support `//@ proc-macro` we need to honor the implicit crate_type==Lib (of the host) here.
        crate_.type_,
        directives.edition.map(|edition| Edition::Raw(edition.bare)),
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client) similar to `extern_prelude_for` here.
        &dependencies,
        flags,
        Strictness::Strict,
    )
    .map_err(Into::into)
}

// FIXME: Support nested auxiliaries!
// FIXME: Detect and reject circular/cyclic auxiliaries.
fn build_compiletest_auxiliary<'a>(
    extern_crate: &ExternCrate<'a>,
    base_path: &Path,
    flags: Flags<'_>,
    flavor: directive::Flavor,
    cx: Context<'_>,
) -> Result<ExternCrate<'a>> {
    let path = match extern_crate {
        ExternCrate::Unnamed { path } => path.map(|path| base_path.join(path)),
        ExternCrate::Named { name, path } => match path {
            Some(path) => path.as_deref().map(|path| base_path.join(path)),
            None => Spanned::sham(base_path.join(name.as_str()).with_extension("rs")),
        },
    };

    // FIXME: unwrap
    let crate_name = CrateName::adjust_and_parse_file_path(&path.bare).unwrap();

    let mut directives = directive::gather(
        path.as_deref(),
        directive::Scope::Base,
        directive::Role::Auxiliary,
        flavor,
        flags.build.revision.as_deref(),
        cx,
    )?;

    let verbatim = mem::take(&mut directives.verbatim).extended(flags.verbatim);
    let flags = Flags { verbatim: verbatim.as_ref(), ..flags };

    command::compile(
        &path.bare,
        crate_name.as_ref(),
        // FIXME: Verify this works with `@compile-flags:--crate-type=proc-macro`
        // FIXME: I don't think it works rn
        CrateType::Lib,
        directives.edition.map(|edition| Edition::Raw(edition.bare)),
        &[],
        flags,
        Strictness::Strict,
    )?;
    // FIXME: Do we need to respect `compile-flags: --crate-name` and adjust `ExternCrate` accordingly?
    Ok(match *extern_crate {
        // FIXME: probably doesn't handle `//@ aux-build: ../file.rs` correctly since `-L.` wouldn't pick it up
        ExternCrate::Unnamed { path } => ExternCrate::Unnamed { path },
        // FIXME: For some reason `compiletest` doesn't support `//@ aux-crate: name=../`
        ExternCrate::Named { name, .. } => {
            // FIXME: unwrap
            let crate_name = CrateName::adjust_and_parse_file_path(&path.bare).unwrap();

            ExternCrate::Named {
                name,
                // FIXME: needs to be relative to the base_path
                // FIXME: layer violation?? should this be the job of mod command?
                path: (name != crate_name.as_ref())
                    .then(|| Spanned::sham(format!("lib{crate_name}.rlib").into())),
            }
        }
    })
}

pub(crate) fn document<'a>(
    mode: DocMode,
    crate_: Crate<'a>,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &crate::interface::DocFlags,
    cx: Context<'_>,
) -> Result<CrateNameCow<'a>> {
    match mode {
        DocMode::Default => document_default(crate_, flags, doc_flags),
        DocMode::CrossCrate => document_cross_crate(crate_, flags, doc_flags),
        DocMode::Compiletest(flavor) => document_compiletest(crate_, flags, doc_flags, flavor, cx),
    }
}

fn document_default<'a>(
    crate_: Crate<'a>,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &crate::interface::DocFlags,
) -> Result<CrateNameCow<'a>> {
    command::document(
        crate_.path,
        crate_.name,
        crate_.type_,
        Some(Edition::Parsed(crate_.edition)),
        extern_prelude_for(crate_.type_),
        flags,
        doc_flags,
        Strictness::Lenient,
    )?;

    Ok(crate_.name.map(Cow::Borrowed))
}

fn document_cross_crate(
    crate_: Crate<'_>,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &crate::interface::DocFlags,
) -> Result<CrateNameCow<'static>> {
    command::compile(
        crate_.path,
        crate_.name,
        crate_.type_.to_non_executable(),
        Some(Edition::Parsed(crate_.edition)),
        extern_prelude_for(crate_.type_),
        flags,
        Strictness::Lenient,
    )?;

    let dependent_crate_name = CrateName::new_unchecked(format!("u_{}", crate_.name));
    let dependent_crate_path =
        crate_.path.with_file_name(dependent_crate_name.as_str()).with_extension("rs");

    if !flags.debug.dry_run && !dependent_crate_path.exists() {
        // While we could omit the `extern crate` declaration in `edition >= Edition::Edition2018`,
        // we would need to recreate the file on each rerun if the edition was 2015 instead of
        // skipping that step since we wouldn't know whether the existing file if applicable was
        // created for a newer edition or not.
        std::fs::write(
            &dependent_crate_path,
            format!("extern crate {0}; pub use {0}::*;\n", crate_.name),
        )?;
    };

    command::document(
        &dependent_crate_path,
        dependent_crate_name.as_ref(),
        default(),
        Some(Edition::Parsed(crate_.edition)),
        &[ExternCrate::Named { name: crate_.name.as_ref(), path: None }],
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
    crate_: Crate<'a>,
    flags: Flags<'_>,
    // FIXME: tempory
    doc_flags: &crate::interface::DocFlags,
    flavor: directive::Flavor,
    cx: Context<'_>,
) -> Result<CrateNameCow<'a>> {
    // FIXME: Do we actually want to treat !`-j` as `rustdoc/` (Scope::HtmlDocCk)
    //        instead of `rustdoc-ui/` ("Scope::Rustdoc")
    let scope = match doc_flags.backend {
        DocBackend::Html => directive::Scope::HtmlDocCk,
        DocBackend::Json => directive::Scope::JsonDocCk,
    };
    let mut directives = directive::gather(
        Spanned::sham(crate_.path),
        scope,
        directive::Role::Principal,
        flavor,
        flags.build.revision.as_deref(),
        cx,
    )?;

    // FIXME: unwrap
    let aux_base_path = LazyCell::new(|| crate_.path.parent().unwrap().join("auxiliary"));

    let dependencies: Vec<_> = directives
        .dependencies
        .iter()
        .map(|dep| {
            document_compiletest_auxiliary(
                dep,
                &aux_base_path,
                directives.build_aux_docs,
                flags,
                doc_flags,
                flavor,
                cx,
            )
        })
        .collect::<Result<_>>()?;

    let verbatim = mem::take(&mut directives.verbatim).extended(flags.verbatim);
    let flags = Flags { verbatim: verbatim.as_ref(), ..flags };

    // FIXME: Should we generally emit a warning when CLI tests anything that *may* lead to clashes down the line?
    //        E.g., on `--crate-name`, `--crate-type` and `--edition`? And what about CLI/env verbatim flags?

    // FIXME: Once this one is an `Option<Edition>` instead, so we can tell if it was explicitly set by the user,
    //        use it to overwrite `directives.edition`.
    let _ = crate_.edition;

    command::document(
        crate_.path,
        crate_.name,
        // FIXME: Once we support `//@ proc-macro` we need to honor the implicit crate_type==Lib (of the host) here.
        crate_.type_,
        directives.edition.map(|edition| Edition::Raw(edition.bare)),
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client) similar to `extern_prelude_for` here.
        &dependencies,
        flags,
        doc_flags,
        Strictness::Strict,
    )?;

    Ok(crate_.name.map(Cow::Borrowed))
}

// FIXME: Support nested auxiliaries!
// FIXME: Detect and reject circular/cyclic auxiliaries.
fn document_compiletest_auxiliary<'a>(
    extern_crate: &ExternCrate<'a>,
    base_path: &Path,
    document: bool,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &crate::interface::DocFlags,
    flavor: directive::Flavor,
    cx: Context<'_>,
) -> Result<ExternCrate<'a>> {
    let path = match extern_crate {
        ExternCrate::Unnamed { path } => path.map(|path| base_path.join(path)),
        ExternCrate::Named { name, path } => match path {
            Some(path) => path.as_deref().map(|path| base_path.join(path)),
            None => Spanned::sham(base_path.join(name.as_str()).with_extension("rs")),
        },
    };

    // FIXME: unwrap
    let crate_name = CrateName::adjust_and_parse_file_path(&path.bare).unwrap();

    // FIXME: DRY
    // FIXME: Do we actually want to treat !`-j` as `rustdoc/` (Scope::HtmlDocCk)
    //        instead of `rustdoc-ui/` ("Scope::Rustdoc")
    let scope = match doc_flags.backend {
        DocBackend::Html => directive::Scope::HtmlDocCk,
        DocBackend::Json => directive::Scope::JsonDocCk,
    };

    let mut directives = directive::gather(
        path.as_deref(),
        scope,
        directive::Role::Auxiliary,
        flavor,
        flags.build.revision.as_deref(),
        cx,
    )?;

    let verbatim = mem::take(&mut directives.verbatim).extended(flags.verbatim);
    let flags = Flags { verbatim: verbatim.as_ref(), ..flags };

    command::compile(
        &path.bare,
        crate_name.as_ref(),
        // FIXME: Verify this works with `@compile-flags:--crate-type=proc-macro`
        // FIXME: I don't think it works rn
        CrateType::Lib,
        directives.edition.map(|edition| Edition::Raw(edition.bare)),
        &[],
        flags,
        Strictness::Strict,
    )?;

    // FIXME: Is this how `//@ build-aux-docs` is supposed to work?
    if document {
        command::document(
            &path.bare,
            crate_name.as_ref(),
            // FIXME: Verify this works with `@compile-flags:--crate-type=proc_macro`
            // FIXME: I don't think it works rn
            default(),
            directives.edition.map(|edition| Edition::Raw(edition.bare)),
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
            let crate_name = CrateName::adjust_and_parse_file_path(&path.bare).unwrap();

            ExternCrate::Named {
                name,
                // FIXME: needs to be relative to the base_path
                // FIXME: layer violation?? should this be the job of mod command?
                path: (name != crate_name.as_ref())
                    .then(|| Spanned::sham(format!("lib{crate_name}.rlib").into())),
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
    Compiletest(directive::Flavor),
}

impl From<DocMode> for Mode {
    fn from(mode: DocMode) -> Self {
        match mode {
            DocMode::Compiletest(_) => Self::Compiletest,
            DocMode::Default | DocMode::CrossCrate => Self::Other,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BuildMode {
    Default,
    Compiletest(directive::Flavor),
}

impl From<BuildMode> for Mode {
    fn from(mode: BuildMode) -> Self {
        match mode {
            BuildMode::Compiletest(_) => Self::Compiletest,
            BuildMode::Default => Self::Other,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Compiletest,
    Other,
}

impl Mode {
    pub(crate) fn edition(self) -> data::Edition {
        match self {
            Self::Compiletest => data::Edition::RUSTC_DEFAULT,
            Self::Other => data::Edition::LATEST_STABLE,
        }
    }
}
