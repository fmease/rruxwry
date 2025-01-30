//! High-level build operations.
//!
//! The low-level build routines are defined in [`crate::build`].

// FIXME: Add explainer about why we use `--print=crate-name` over `-o` (crate type nuisance; rustdoc no likey).

// FIXME: Create test for `//@ compile-flags: --extern name` + aux-build
// FIXME: Create test for `//@ compile-flags: --test`.

use crate::{
    build::{
        self, CompileOptions, DocOptions, Edition, ExternCrate, ImplyUnstableOptions, Options,
        VerbatimOptions,
    },
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
    op: Operation,
    krate: Crate<'_>,
    opts: Options<'_>,
    cx: Context<'_>,
) -> Result<()> {
    match op {
        Operation::Compile { mode, run, options: c_opts } => {
            compile(mode, run, krate, opts, &c_opts, cx)
        }
        Operation::Document { mode, open, options: d_opts } => {
            document(mode, open, krate, opts, &d_opts, cx)
        }
    }
}

fn compile(
    mode: CompileMode,
    run: Run,
    krate: Crate<'_>,
    opts: Options<'_>,
    c_opts: &CompileOptions,
    cx: Context<'_>,
) -> Result {
    match mode {
        CompileMode::Default => compile_default(krate, opts, c_opts, run),
        CompileMode::Compiletest(flavor) => {
            compile_compiletest(krate, opts, c_opts, flavor, run, cx)
        }
    }
}

fn run(
    krate: Crate<'_, Option<build::Edition<'_>>>,
    opts: Options<'_>,
    run_v_opts: VerbatimOptions<'_>,
) -> Result {
    // FIXME: Explainer
    let crate_name = build::query_crate_name(krate, opts)?;
    let mut path: PathBuf = [".", &crate_name].into_iter().collect();
    path.set_extension(std::env::consts::EXE_EXTENSION);

    build::run(&path, run_v_opts, opts.debug).map_err(|error| {
        self::error(fmt!("failed to run the built binary `{}`", path.display()))
            .note(fmt!("{error}"))
            .finish()
    })?;
    Ok(())
}

fn compile_default(
    krate: Crate<'_>,
    opts: Options<'_>,
    c_opts: &CompileOptions,
    run: Run,
) -> Result {
    let krate = Crate { edition: Some(Edition::Parsed(krate.edition)), ..krate };

    build::perform(
        build::Engine::Rustc(c_opts),
        krate,
        extern_prelude_for(krate.typ),
        opts,
        ImplyUnstableOptions::Yes,
    )?;

    match run {
        Run::Yes => self::run(krate, opts, default()),
        Run::No => Ok(()),
    }
}

fn compile_compiletest(
    krate: Crate<'_>,
    opts: Options<'_>,
    c_opts: &CompileOptions,
    flavor: directive::Flavor,
    run: Run,
    cx: Context<'_>,
) -> Result {
    let mut directives = directive::gather(
        Spanned::sham(krate.path),
        directive::Scope::Base,
        directive::Role::Principal,
        flavor,
        opts.build.revision.as_deref(),
        cx,
    )?;

    // FIXME: unwrap
    let aux_base_path = LazyCell::new(|| krate.path.parent().unwrap().join("auxiliary"));

    let deps: Vec<_> = directives
        .dependencies
        .iter()
        .map(|dep| compile_compiletest_auxiliary(dep, &aux_base_path, opts, c_opts, flavor, cx))
        .collect::<Result<_>>()?;

    directives.build_verbatim.extend(opts.verbatim);
    // FIXME: Should we generally emit a warning when CLI tests anything that *may* lead to clashes down the line?
    //        E.g., on `--crate-name`, `--crate-type` and `--edition`? And what about CLI/env verbatim opts?

    // FIXME: Once this one is an `Option<Edition>` instead, so we can tell if it was explicitly set by the user,
    //        use it to overwrite `directives.edition`.
    let _ = krate.edition;

    // FIXME: Once we support `//@ proc-macro` we need to reflect that in the crate type.
    let krate =
        Crate { edition: directives.edition.map(|edition| Edition::Raw(edition.bare)), ..krate };

    let opts = Options { verbatim: directives.build_verbatim.as_ref(), ..opts };

    build::perform(
        build::Engine::Rustc(c_opts),
        krate,
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client) similar to `extern_prelude_for` here.
        &deps,
        opts,
        ImplyUnstableOptions::No,
    )?;

    match run {
        Run::Yes => self::run(krate, opts, directives.run_verbatim.as_ref()),
        Run::No => Ok(()),
    }
}

// FIXME: Support nested auxiliaries!
// FIXME: Detect and reject circular/cyclic auxiliaries.
fn compile_compiletest_auxiliary<'a>(
    extern_crate: &ExternCrate<'a>,
    base_path: &Path,
    opts: Options<'_>,
    c_opts: &CompileOptions,
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
        opts.build.revision.as_deref(),
        cx,
    )?;

    directives.build_verbatim.extend(opts.verbatim);
    build::perform(
        build::Engine::Rustc(c_opts),
        Crate {
            path: &path.bare,
            name: crate_name.as_ref(),
            // FIXME: Verify this works with `@compile-flags:--crate-type=proc-macro`
            // FIXME: I don't think it works rn
            typ: CrateType::Lib,
            edition: directives.edition.map(|edition| Edition::Raw(edition.bare)),
        },
        &[],
        Options { verbatim: directives.build_verbatim.as_ref(), ..opts },
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
    krate: Crate<'_>,
    opts: Options<'_>,
    d_opts: &DocOptions,
    cx: Context<'_>,
) -> Result<()> {
    match mode {
        DocMode::Default => document_default(krate, opts, d_opts, open),
        DocMode::CrossCrate => document_cross_crate(krate, opts, d_opts, open),
        DocMode::Compiletest(flavor) => document_compiletest(krate, opts, d_opts, flavor, open, cx),
    }
}

fn open(krate: Crate<'_, Option<build::Edition<'_>>>, opts: Options<'_>) -> Result<()> {
    let crate_name = build::query_crate_name(krate, opts)?;
    let path = format!("./doc/{crate_name}/index.html");

    build::open(Path::new(&path), opts.debug).map_err(|error| {
        self::error(fmt!("failed to open the generated docs in a browser"))
            .note(fmt!("{error}"))
            .finish()
    })?;
    Ok(())
}

fn document_default(
    krate: Crate<'_>,
    opts: Options<'_>,
    d_opts: &DocOptions,
    open: Open,
) -> Result<()> {
    let krate = Crate { edition: Some(Edition::Parsed(krate.edition)), ..krate };

    build::perform(
        build::Engine::Rustdoc(d_opts),
        krate,
        extern_prelude_for(krate.typ),
        opts,
        ImplyUnstableOptions::Yes,
    )?;

    match open {
        Open::Yes => self::open(krate, opts),
        Open::No => Ok(()),
    }
}

fn document_cross_crate(
    krate: Crate<'_>,
    opts: Options<'_>,
    d_opts: &DocOptions,
    open: Open,
) -> Result<()> {
    build::perform(
        build::Engine::Rustc(&CompileOptions { check: false }),
        Crate {
            typ: krate.typ.to_non_executable(),
            edition: Some(Edition::Parsed(krate.edition)),
            ..krate
        },
        extern_prelude_for(krate.typ),
        opts,
        ImplyUnstableOptions::Yes,
    )?;

    let root_crate_name = CrateName::new_unchecked(format!("u_{}", krate.name));
    let root_crate_path = krate.path.with_file_name(root_crate_name.as_str()).with_extension("rs");

    if !opts.debug.dry_run && !root_crate_path.exists() {
        // While we could omit the `extern crate` declaration in `edition >= Edition::Edition2018`,
        // we would need to recreate the file on each rerun if the edition was 2015 instead of
        // skipping that step since we wouldn't know whether the existing file if applicable was
        // created for a newer edition or not.
        std::fs::write(
            &root_crate_path,
            format!("extern crate {0}; pub use {0}::*;\n", krate.name),
        )?;
    };

    let deps = &[ExternCrate::Named { name: krate.name.as_ref(), path: None }];

    let krate = Crate {
        path: &root_crate_path,
        name: root_crate_name.as_ref(),
        typ: default(),
        edition: Some(Edition::Parsed(krate.edition)),
    };

    build::perform(build::Engine::Rustdoc(d_opts), krate, deps, opts, ImplyUnstableOptions::Yes)?;

    match open {
        Open::Yes => self::open(krate, opts),
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
    krate: Crate<'_>,
    opts: Options<'_>,
    // FIXME: tempory
    d_opts: &DocOptions,
    flavor: directive::Flavor,
    open: Open,
    cx: Context<'_>,
) -> Result<()> {
    // FIXME: Do we actually want to treat !`-j` as `rustdoc/` (Scope::HtmlDocCk)
    //        instead of `rustdoc-ui/` ("Scope::Rustdoc")
    let scope = match d_opts.backend {
        DocBackend::Html => directive::Scope::HtmlDocCk,
        DocBackend::Json => directive::Scope::JsonDocCk,
    };
    let mut directives = directive::gather(
        Spanned::sham(krate.path),
        scope,
        directive::Role::Principal,
        flavor,
        opts.build.revision.as_deref(),
        cx,
    )?;

    // FIXME: unwrap
    let aux_base_path = LazyCell::new(|| krate.path.parent().unwrap().join("auxiliary"));

    let dependencies: Vec<_> = directives
        .dependencies
        .iter()
        .map(|dep| {
            document_compiletest_auxiliary(
                dep,
                &aux_base_path,
                directives.build_aux_docs,
                opts,
                d_opts,
                flavor,
                cx,
            )
        })
        .collect::<Result<_>>()?;

    directives.build_verbatim.extend(opts.verbatim);
    // FIXME: Should we generally emit a warning when CLI tests anything that *may* lead to clashes down the line?
    //        E.g., on `--crate-name`, `--crate-type` and `--edition`? And what about CLI/env verbatim opts?

    // FIXME: Once this one is an `Option<Edition>` instead, so we can tell if it was explicitly set by the user,
    //        use it to overwrite `directives.edition`.
    let _ = krate.edition;

    // FIXME: Once we support `//@ proc-macro` we need to reflect that in the crate type.
    let krate =
        Crate { edition: directives.edition.map(|edition| Edition::Raw(edition.bare)), ..krate };

    build::perform(
        build::Engine::Rustdoc(d_opts),
        krate,
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client) similar to `extern_prelude_for` here.
        &dependencies,
        Options { verbatim: directives.build_verbatim.as_ref(), ..opts },
        ImplyUnstableOptions::No,
    )?;

    match open {
        Open::Yes => self::open(krate, opts),
        Open::No => Ok(()),
    }
}

// FIXME: Support nested auxiliaries!
// FIXME: Detect and reject circular/cyclic auxiliaries.
fn document_compiletest_auxiliary<'a>(
    extern_crate: &ExternCrate<'a>,
    base_path: &Path,
    document: bool,
    opts: Options<'_>,
    d_opts: &DocOptions,
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
    let scope = match d_opts.backend {
        DocBackend::Html => directive::Scope::HtmlDocCk,
        DocBackend::Json => directive::Scope::JsonDocCk,
    };

    let mut directives = directive::gather(
        path.as_deref(),
        scope,
        directive::Role::Auxiliary,
        flavor,
        opts.build.revision.as_deref(),
        cx,
    )?;

    directives.build_verbatim.extend(opts.verbatim);
    let opts = Options { verbatim: directives.build_verbatim.as_ref(), ..opts };

    build::perform(
        build::Engine::Rustc(&CompileOptions { check: false }),
        Crate {
            path: &path.bare,
            name: crate_name.as_ref(),
            // FIXME: Verify this works with `@compile-flags:--crate-type=proc-macro`
            // FIXME: I don't think it works rn
            typ: CrateType::Lib,
            edition: directives.edition.map(|edition| Edition::Raw(edition.bare)),
        },
        &[],
        opts,
        ImplyUnstableOptions::No,
    )?;

    // FIXME: Is this how `//@ build-aux-docs` is supposed to work?
    if document {
        build::perform(
            build::Engine::Rustdoc(d_opts),
            Crate {
                path: &path.bare,
                name: crate_name.as_ref(),
                // FIXME: Verify this works with `@compile-flags:--crate-type=proc_macro`
                // FIXME: I don't think it works rn
                typ: default(),
                edition: directives.edition.map(|edition| Edition::Raw(edition.bare)),
            },
            &[],
            opts,
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
    Compile { mode: CompileMode, run: Run, options: CompileOptions },
    Document { mode: DocMode, open: Open, options: DocOptions },
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
