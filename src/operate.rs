//! High-level build operations.
//!
//! The low-level build routines are defined in [`crate::build`].

// FIXME: Add explainer about why we use `--print=crate-name` over `-o` (crate type nuisance; rustdoc no likey).

// FIXME: Create test for `//@ compile-flags: --extern name` + aux-build
// FIXME: Create test for `//@ compile-flags: --test`.

use crate::{
    build::{
        self, CompileOptions, DocOptions, ExternCrate, ImplyUnstableOptions, Options,
        VerbatimOptions,
    },
    context::Context,
    data::{Crate, CrateName, CrateType, DocBackend, Edition},
    diagnostic::error,
    directive,
    error::Result,
    fmt,
    source::Spanned,
    utility::default,
};
use std::{
    borrow::Cow,
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

fn run(krate: Crate<'_>, opts: Options<'_>, run_v_opts: VerbatimOptions<'_>) -> Result {
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
    let krate = Crate { edition: krate.edition.or(Some(Edition::LATEST_STABLE)), ..krate };

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

    // FIXME: Once we support `//@ proc-macro` we need to reflect that in the crate type.
    let krate = Crate {
        edition: krate.edition.or(directives.edition.map(|edition| Edition::Unknown(edition.bare))),
        ..krate
    };

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
    // FIXME: Do we actually want to pass along these opts? Arguably they belong to the root crate.
    //        The status quo is inconsistent because we don't honor the krate.edition (which is
    //        also just an "Option") etc. (except krate.name obv).
    //        At the very least, passing this along should be behind an option itself.
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
            // FIXME: Or does compiletest do something 'smarter'?
            name: None,
            // FIXME: Make this "dylib" instead unless directives.no_prefer_dynamic then it should be..None?
            typ: Some(CrateType("lib")),
            edition: directives.edition.map(|edition| Edition::Unknown(edition.bare)),
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
            // FIXME: do we *need* to do this???
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

fn open(krate: Crate<'_>, opts: Options<'_>) -> Result<()> {
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
    let krate = Crate { edition: krate.edition.or(Some(Edition::LATEST_STABLE)), ..krate };

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
    let edition = krate.edition.or(Some(Edition::LATEST_STABLE));

    build::perform(
        build::Engine::Rustc(&CompileOptions { check: false }),
        // FIXME: Should we check for `krate.typ=="bin"` and reject it? Possibly leads to a nicer UX.
        Crate { typ: krate.typ, edition, ..krate },
        extern_prelude_for(krate.typ),
        opts,
        ImplyUnstableOptions::Yes,
    )?;

    // FIXME: This `unwrap` is obviously reachable (e.g., on `rrc '%$?'`)
    let crate_name: CrateName<Cow<'_, _>> = krate.name.map_or_else(
        || CrateName::adjust_and_parse_file_path(krate.path).unwrap().into(),
        Into::into,
    );

    let root_crate_name = CrateName::new_unchecked(format!("u_{crate_name}"));
    let root_crate_path = krate.path.with_file_name(root_crate_name.as_str()).with_extension("rs");

    if !opts.debug.dry_run && !root_crate_path.exists() {
        // While we could omit the `extern crate` declaration in `edition >= Edition::Edition2018`,
        // we would need to recreate the file on each rerun if the edition was 2015 instead of
        // skipping that step since we wouldn't know whether the existing file if applicable was
        // created for a newer edition or not.
        std::fs::write(
            &root_crate_path,
            format!("extern crate {crate_name}; pub use {crate_name}::*;\n"),
        )?;
    };

    let deps = &[ExternCrate::Named { name: crate_name.as_ref(), path: None }];

    let krate =
        Crate { path: &root_crate_path, name: Some(root_crate_name.as_ref()), typ: None, edition };

    build::perform(build::Engine::Rustdoc(d_opts), krate, deps, opts, ImplyUnstableOptions::Yes)?;

    match open {
        Open::Yes => self::open(krate, opts),
        Open::No => Ok(()),
    }
}

fn extern_prelude_for(typ: Option<CrateType>) -> &'static [ExternCrate<'static>] {
    match typ {
        // For convenience and just like Cargo we add `proc_macro` to the external prelude.
        Some(CrateType("proc-macro")) => &[ExternCrate::Named {
            name: const { CrateName::new_unchecked("proc_macro") },
            path: None,
        }],
        _ => default(),
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

    // FIXME: Once we support `//@ proc-macro` we need to reflect that in the crate type.
    let krate = Crate {
        edition: krate.edition.or(directives.edition.map(|edition| Edition::Unknown(edition.bare))),
        ..krate
    };

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
    // FIXME: Do we actually want to pass along these opts? Arguably they belong to the root crate.
    //        The status quo is inconsistent because we don't honor the krate.edition (which is
    //        also just an "Option") etc. (except krate.name obv).
    //        At the very least, passing this along should be behind an option itself.
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
            // FIXME: Does compiletest do something 'smarter'?
            name: None,
            // FIXME: Make this "dylib" instead unless directives.no_prefer_dynamic then it should be..None?
            typ: Some(CrateType("lib")),
            edition: directives.edition.map(|edition| Edition::Unknown(edition.bare)),
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
                // FIXME: Does compiletest do something 'smarter'?
                name: None,
                // FIXME: Should this also be Some("dylib") unless directives.no_prefer_dynamic?
                typ: None,
                edition: directives.edition.map(|edition| Edition::Unknown(edition.bare)),
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
            // FIXME: Is this strictly necessary?
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

#[derive(Clone, Copy)]
pub(crate) enum CompileMode {
    Default,
    Compiletest(directive::Flavor),
}

#[derive(Clone, Copy)]
pub(crate) enum Run {
    Yes,
    No,
}

#[derive(Clone, Copy)]
pub(crate) enum DocMode {
    Default,
    CrossCrate,
    Compiletest(directive::Flavor),
}

#[derive(Clone, Copy)]
pub(crate) enum Open {
    Yes,
    No,
}
