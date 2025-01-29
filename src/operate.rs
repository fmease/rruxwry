//! High-level build operations.
//!
//! The low-level build routines are defined in [`crate::build`].

// FIXME: Add explainer about why we use `--print=crate-name` over `-o` (crate type nuisance; rustdoc no likey).

// FIXME: Create test for `//@ compile-flags: --extern name` + aux-build
// FIXME: Create test for `//@ compile-flags: --test`.

use crate::{
    build::{self, DocFlags, Edition, ExternCrate, Flags, ImplyUnstableOptions, VerbatimData},
    context::Context,
    data::{self, Crate, CrateName, CrateType, DocBackend},
    diagnostic::error,
    directive,
    error::Result,
    fmt,
    source::Spanned,
    utility::default,
};
use std::{
    cell::LazyCell,
    path::{Path, PathBuf},
};

pub(crate) fn perform(
    operation: Operation,
    crate_: Crate<'_>,
    flags: Flags<'_>,
    cx: Context<'_>,
) -> Result<()> {
    match operation {
        Operation::Compile { mode, run } => compile(mode, run, crate_, flags, cx),
        Operation::Document { mode, open, flags: doc_flags } => {
            document(mode, open, crate_, flags, &doc_flags, cx)
        }
    }
}

fn compile(
    mode: CompileMode,
    run: Run,
    crate_: Crate<'_>,
    flags: Flags<'_>,
    cx: Context<'_>,
) -> Result {
    match mode {
        CompileMode::Default => compile_default(crate_, flags, run),
        CompileMode::Compiletest(flavor) => compile_compiletest(crate_, flags, flavor, run, cx),
    }
}

fn run(
    crate_: Crate<'_, Option<build::Edition<'_>>>,
    flags: Flags<'_>,
    run_verbatim: VerbatimData<'_>,
) -> Result {
    // FIXME: Explainer
    let crate_name = build::query_crate_name(crate_, flags)?;
    let mut path: PathBuf = [".", &crate_name].into_iter().collect();
    path.set_extension(std::env::consts::EXE_EXTENSION);

    build::run(&path, run_verbatim, flags.debug).map_err(|error| {
        self::error(fmt!("failed to run the built binary `{}`", path.display()))
            .note(fmt!("{error}"))
            .finish()
    })?;
    Ok(())
}

fn compile_default(crate_: Crate<'_>, flags: Flags<'_>, run: Run) -> Result {
    let crate_ = Crate { edition: Some(Edition::Parsed(crate_.edition)), ..crate_ };

    build::perform(
        build::Engine::Rustc,
        crate_,
        extern_prelude_for(crate_.type_),
        flags,
        ImplyUnstableOptions::Yes,
    )?;

    match run {
        Run::Yes => self::run(crate_, flags, default()),
        Run::No => Ok(()),
    }
}

fn compile_compiletest(
    crate_: Crate<'_>,
    flags: Flags<'_>,
    flavor: directive::Flavor,
    run: Run,
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
        .map(|dep| compile_compiletest_auxiliary(dep, &aux_base_path, flags, flavor, cx))
        .collect::<Result<_>>()?;

    directives.build_verbatim.extend(flags.verbatim);
    // FIXME: Should we generally emit a warning when CLI tests anything that *may* lead to clashes down the line?
    //        E.g., on `--crate-name`, `--crate-type` and `--edition`? And what about CLI/env verbatim flags?

    // FIXME: Once this one is an `Option<Edition>` instead, so we can tell if it was explicitly set by the user,
    //        use it to overwrite `directives.edition`.
    let _ = crate_.edition;

    // FIXME: Once we support `//@ proc-macro` we need to reflect that in the crate type.
    let crate_ =
        Crate { edition: directives.edition.map(|edition| Edition::Raw(edition.bare)), ..crate_ };

    let flags = Flags { verbatim: directives.build_verbatim.as_ref(), ..flags };

    build::perform(
        build::Engine::Rustc,
        crate_,
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client) similar to `extern_prelude_for` here.
        &dependencies,
        flags,
        ImplyUnstableOptions::No,
    )?;

    match run {
        Run::Yes => self::run(crate_, flags, directives.run_verbatim.as_ref()),
        Run::No => Ok(()),
    }
}

// FIXME: Support nested auxiliaries!
// FIXME: Detect and reject circular/cyclic auxiliaries.
fn compile_compiletest_auxiliary<'a>(
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

    directives.build_verbatim.extend(flags.verbatim);
    build::perform(
        build::Engine::Rustc,
        Crate {
            path: &path.bare,
            name: crate_name.as_ref(),
            // FIXME: Verify this works with `@compile-flags:--crate-type=proc-macro`
            // FIXME: I don't think it works rn
            type_: CrateType::Lib,
            edition: directives.edition.map(|edition| Edition::Raw(edition.bare)),
        },
        &[],
        Flags { verbatim: directives.build_verbatim.as_ref(), ..flags },
        ImplyUnstableOptions::No,
    )?;

    // FIXME: Clean up this junk!
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

fn document(
    mode: DocMode,
    open: Open,
    crate_: Crate<'_>,
    flags: Flags<'_>,
    doc_flags: &build::DocFlags,
    cx: Context<'_>,
) -> Result<()> {
    match mode {
        DocMode::Default => document_default(crate_, flags, doc_flags, open),
        DocMode::CrossCrate => document_cross_crate(crate_, flags, doc_flags, open),
        DocMode::Compiletest(flavor) => {
            document_compiletest(crate_, flags, doc_flags, flavor, open, cx)
        }
    }
}

fn open(crate_: Crate<'_, Option<build::Edition<'_>>>, flags: Flags<'_>) -> Result<()> {
    let crate_name = build::query_crate_name(crate_, flags)?;
    let path = format!("./doc/{crate_name}/index.html");

    build::open(Path::new(&path), flags.debug).map_err(|error| {
        self::error(fmt!("failed to open the generated docs in a browser"))
            .note(fmt!("{error}"))
            .finish()
    })?;
    Ok(())
}

fn document_default(
    crate_: Crate<'_>,
    flags: Flags<'_>,
    doc_flags: &build::DocFlags,
    open: Open,
) -> Result<()> {
    let crate_ = Crate { edition: Some(Edition::Parsed(crate_.edition)), ..crate_ };

    build::perform(
        build::Engine::Rustdoc(doc_flags),
        crate_,
        extern_prelude_for(crate_.type_),
        flags,
        ImplyUnstableOptions::Yes,
    )?;

    match open {
        Open::Yes => self::open(crate_, flags),
        Open::No => Ok(()),
    }
}

fn document_cross_crate(
    crate_: Crate<'_>,
    flags: Flags<'_>,
    doc_flags: &build::DocFlags,
    open: Open,
) -> Result<()> {
    build::perform(
        build::Engine::Rustc,
        Crate {
            type_: crate_.type_.to_non_executable(),
            edition: Some(Edition::Parsed(crate_.edition)),
            ..crate_
        },
        extern_prelude_for(crate_.type_),
        flags,
        ImplyUnstableOptions::Yes,
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

    let dependencies = &[ExternCrate::Named { name: crate_.name.as_ref(), path: None }];

    let crate_ = Crate {
        path: &dependent_crate_path,
        name: dependent_crate_name.as_ref(),
        type_: default(),
        edition: Some(Edition::Parsed(crate_.edition)),
    };

    build::perform(
        build::Engine::Rustdoc(doc_flags),
        crate_,
        dependencies,
        flags,
        ImplyUnstableOptions::Yes,
    )?;

    match open {
        Open::Yes => self::open(crate_, flags),
        Open::No => Ok(()),
    }
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

fn document_compiletest(
    crate_: Crate<'_>,
    flags: Flags<'_>,
    // FIXME: tempory
    doc_flags: &build::DocFlags,
    flavor: directive::Flavor,
    open: Open,
    cx: Context<'_>,
) -> Result<()> {
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

    directives.build_verbatim.extend(flags.verbatim);
    // FIXME: Should we generally emit a warning when CLI tests anything that *may* lead to clashes down the line?
    //        E.g., on `--crate-name`, `--crate-type` and `--edition`? And what about CLI/env verbatim flags?

    // FIXME: Once this one is an `Option<Edition>` instead, so we can tell if it was explicitly set by the user,
    //        use it to overwrite `directives.edition`.
    let _ = crate_.edition;

    // FIXME: Once we support `//@ proc-macro` we need to reflect that in the crate type.
    let crate_ =
        Crate { edition: directives.edition.map(|edition| Edition::Raw(edition.bare)), ..crate_ };

    build::perform(
        build::Engine::Rustdoc(doc_flags),
        crate_,
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client) similar to `extern_prelude_for` here.
        &dependencies,
        Flags { verbatim: directives.build_verbatim.as_ref(), ..flags },
        ImplyUnstableOptions::No,
    )?;

    match open {
        Open::Yes => self::open(crate_, flags),
        Open::No => Ok(()),
    }
}

// FIXME: Support nested auxiliaries!
// FIXME: Detect and reject circular/cyclic auxiliaries.
fn document_compiletest_auxiliary<'a>(
    extern_crate: &ExternCrate<'a>,
    base_path: &Path,
    document: bool,
    flags: Flags<'_>,
    // FIXME: temporary
    doc_flags: &build::DocFlags,
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

    directives.build_verbatim.extend(flags.verbatim);
    let flags = Flags { verbatim: directives.build_verbatim.as_ref(), ..flags };

    build::perform(
        build::Engine::Rustc,
        Crate {
            path: &path.bare,
            name: crate_name.as_ref(),
            // FIXME: Verify this works with `@compile-flags:--crate-type=proc-macro`
            // FIXME: I don't think it works rn
            type_: CrateType::Lib,
            edition: directives.edition.map(|edition| Edition::Raw(edition.bare)),
        },
        &[],
        flags,
        ImplyUnstableOptions::No,
    )?;

    // FIXME: Is this how `//@ build-aux-docs` is supposed to work?
    if document {
        build::perform(
            build::Engine::Rustdoc(doc_flags),
            Crate {
                path: &path.bare,
                name: crate_name.as_ref(),
                // FIXME: Verify this works with `@compile-flags:--crate-type=proc_macro`
                // FIXME: I don't think it works rn
                type_: default(),
                edition: directives.edition.map(|edition| Edition::Raw(edition.bare)),
            },
            &[],
            flags,
            ImplyUnstableOptions::No,
        )?;
    }

    // FIXME: Clean up this junk!
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

pub(crate) enum Operation {
    Compile { mode: CompileMode, run: Run },
    Document { mode: DocMode, open: Open, flags: DocFlags },
}

impl Operation {
    // FIXME: Make this private once possible
    pub(crate) fn mode(&self) -> Mode {
        match *self {
            Self::Compile { mode, .. } => mode.into(),
            Self::Document { mode, .. } => mode.into(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Run {
    Yes,
    No,
}

#[derive(Clone, Copy)]
pub(crate) enum Open {
    Yes,
    No,
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
pub(crate) enum CompileMode {
    Default,
    Compiletest(directive::Flavor),
}

impl From<CompileMode> for Mode {
    fn from(mode: CompileMode) -> Self {
        match mode {
            CompileMode::Compiletest(_) => Self::Compiletest,
            CompileMode::Default => Self::Other,
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
