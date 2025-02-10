//! High-level build operations.
//!
//! The low-level build routines are defined in [`crate::build`].

// FIXME: Add explainer about why we use `--print=crate-name` over `-o` (crate type nuisance; rustdoc no likey).

// FIXME: Create test for `//@ compile-flags: --extern name` + aux-build
// FIXME: Create test for `//@ compile-flags: --test`.

use crate::{
    build::{
        self, CompileOptions, DocOptions, Engine, ExternCrate, ImplyUnstableOptions, Options,
        VerbatimOptions,
    },
    context::Context,
    data::{Crate, CrateName, CrateType, DocBackend, Edition},
    diagnostic::{error, fmt},
    directive,
    error::Result,
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

fn compile<'a>(
    mode: CompileMode,
    run: Run,
    krate: Crate<'a>,
    mut opts: Options<'a>,
    c_opts: &CompileOptions,
    cx: Context<'a>,
) -> Result {
    let engine = Engine::Rustc(c_opts);
    let (krate, run_v_opts) = match mode {
        CompileMode::Default => {
            let krate = build_default(engine, krate, &opts)?;
            (krate, default())
        }
        // FIXME: _test
        CompileMode::DirectiveDriven(flavor, _test) => {
            build_directive_driven(engine, krate, &mut opts, flavor, cx)?
        }
    };
    match run {
        Run::Yes => self::run(krate, &opts, &run_v_opts),
        Run::No => Ok(()),
    }
}

fn run(krate: Crate<'_>, opts: &Options<'_>, run_v_opts: &VerbatimOptions<'_>) -> Result {
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

fn document<'a>(
    mode: DocMode,
    open: Open,
    krate: Crate<'a>,
    mut opts: Options<'a>,
    d_opts: &DocOptions,
    cx: Context<'a>,
) -> Result<()> {
    let engine = Engine::Rustdoc(d_opts);
    let (krate, opts) = match mode {
        DocMode::Default => {
            let krate = build_default(engine, krate, &opts)?;
            (krate, opts)
        }
        DocMode::CrossCrate => return document_cross_crate(krate, &opts, d_opts, open),
        // FIXME: _test
        DocMode::DirectiveDriven(flavor, _test) => {
            let (krate, _) = build_directive_driven(engine, krate, &mut opts, flavor, cx)?;
            (krate, opts)
        }
    };
    match open {
        Open::Yes => self::open(krate, &opts),
        Open::No => Ok(()),
    }
}

fn open(krate: Crate<'_>, opts: &Options<'_>) -> Result<()> {
    let crate_name = build::query_crate_name(krate, opts)?;
    let path = format!("./doc/{crate_name}/index.html");

    build::open(Path::new(&path), opts.debug).map_err(|error| {
        self::error(fmt!("failed to open the generated docs in a browser"))
            .note(fmt!("{error}"))
            .finish()
    })?;
    Ok(())
}

fn document_cross_crate(
    krate: Crate<'_>,
    opts: &Options<'_>,
    d_opts: &DocOptions,
    open: Open,
) -> Result<()> {
    let krate = Crate { typ: krate.typ.or(Some(CrateType("lib"))), ..krate };
    let krate = build_default(Engine::Rustc(&CompileOptions { check_only: false }), krate, opts)?;

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
        Crate { path: &root_crate_path, name: Some(root_crate_name.as_ref()), typ: None, ..krate };

    build::perform(Engine::Rustdoc(d_opts), krate, deps, opts, ImplyUnstableOptions::Yes)?;

    // FIXME: Move this out of this function into the caller `document` to further simplify things
    match open {
        Open::Yes => self::open(krate, opts),
        Open::No => Ok(()),
    }
}

fn build_default<'a>(
    engine: Engine<'_>,
    krate: Crate<'a>,
    opts: &Options<'_>,
) -> Result<Crate<'a>> {
    let krate = Crate { edition: krate.edition.or(Some(Edition::LATEST_STABLE)), ..krate };
    let deps: &[_] = match krate.typ {
        // For convenience and just like Cargo we add `proc_macro` to the external prelude.
        Some(CrateType("proc-macro")) => &[ExternCrate::Named {
            name: const { CrateName::new_unchecked("proc_macro") },
            path: None,
        }],
        _ => &[],
    };
    build::perform(engine, krate, deps, opts, ImplyUnstableOptions::Yes)?;
    Ok(krate)
}

fn build_directive_driven<'a>(
    engine: Engine<'_>,
    krate: Crate<'a>,
    opts: &mut Options<'a>,
    flavor: directive::Flavor,
    cx: Context<'a>,
) -> Result<(Crate<'a>, VerbatimOptions<'a>)> {
    let directives = directive::gather(
        Spanned::sham(krate.path),
        scope(engine),
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
        .map(|dep| {
            compile_auxiliary(
                dep,
                &aux_base_path,
                engine,
                opts.clone(),
                directives.build_aux_docs,
                flavor,
                cx,
            )
        })
        .collect::<Result<_>>()?;

    // FIXME: Once we support `//@ proc-macro` we need to reflect that in the crate type.
    let krate = Crate {
        edition: krate.edition.or(directives.edition.map(|edition| Edition::Unknown(edition.bare))),
        ..krate
    };

    opts.verbatim.extend(directives.build_verbatim);

    build::perform(
        engine,
        krate,
        // FIXME: Once we support `//@ proc-macro` we need to add `proc_macro` (to the client).
        &deps,
        opts,
        ImplyUnstableOptions::No,
    )?;
    Ok((krate, directives.run_verbatim))
}

// FIXME: Support nested auxiliaries!
// FIXME: Detect and reject circular/cyclic auxiliaries.
fn compile_auxiliary<'a>(
    extern_crate: &ExternCrate<'a>,
    base_path: &Path,
    engine: Engine<'_>,
    // FIXME: Do we actually want to pass along *all* of these opts?
    //        Arguably some of them belong to the root crate only (e.g. crate name).
    //        On top of that, the status quo is inconsistent because
    //        we don't honor the edition (which is also just an "option").
    //        Some options should however be inherited: toolchain, cfgs, rev,
    //        debug. Should subset vs. all be a CLI option?
    mut opts: Options<'a>,
    build_aux_docs: bool,
    flavor: directive::Flavor,
    cx: Context<'a>,
) -> Result<ExternCrate<'a>> {
    let path = match extern_crate {
        ExternCrate::Unnamed { path } => path.map(|path| base_path.join(path)),
        ExternCrate::Named { name, path } => match path {
            Some(path) => path.as_deref().map(|path| base_path.join(path)),
            None => Spanned::sham(base_path.join(name.as_str()).with_extension("rs")),
        },
    };

    let directives = directive::gather(
        path.as_deref(),
        scope(engine),
        directive::Role::Auxiliary,
        flavor,
        opts.build.revision.as_deref(),
        cx,
    )?;

    opts.verbatim.extend(directives.build_verbatim);

    let krate = Crate {
        path: &path.bare,
        // FIXME: Does compiletest do something 'smarter'?
        name: None,
        typ: None,
        edition: directives.edition.map(|edition| Edition::Unknown(edition.bare)),
    };

    build::perform(
        match engine {
            // FIXME: Does this actually work as expected wrt. to check-only?
            //        Does this lead to all crates in the dependency graph to
            //        get checked-only and everything working out (linking correctly etc)?
            //        I suspect is doesn't because we need to s%/rlib/rmeta/
            Engine::Rustc(_) => engine,
            // FIXME: Wait, would check_only=true also work and be better?
            Engine::Rustdoc(_) => Engine::Rustc(&CompileOptions { check_only: false }),
        },
        Crate {
            // FIXME: Make this "dylib" instead unless directives.no_prefer_dynamic then it should be..None?
            typ: Some(CrateType("lib")),
            ..krate
        },
        &[],
        &opts,
        ImplyUnstableOptions::No,
    )?;

    // FIXME: Is this how `//@ build-aux-docs` is supposed to work?
    if build_aux_docs && let Engine::Rustdoc(d_opts) = engine {
        build::perform(
            Engine::Rustdoc(d_opts),
            // FIXME: Should typ also be Some("dylib") unless directives.no_prefer_dynamic?
            krate,
            &[],
            &opts,
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
            // FIXME: do we *need* to do this???
            let crate_name = CrateName::adjust_and_parse_file_path(&path.bare).unwrap();

            ExternCrate::Named {
                name,
                // FIXME: needs to be relative to the base_path
                // FIXME: layer violation?? should this be the job of crate::build?
                path: (name != crate_name.as_ref())
                    .then(|| Spanned::sham(format!("lib{crate_name}.rlib").into())),
            }
        }
    })
}

fn scope(engine: Engine<'_>) -> directive::Scope {
    match engine {
        Engine::Rustc(_) => directive::Scope::Base,
        // FIXME: Do we actually want to treat !`-j` as `rustdoc/` (Scope::HtmlDocCk)
        //        instead of `rustdoc-ui/` ("Scope::Rustdoc")
        Engine::Rustdoc(d_opts) => match d_opts.backend {
            DocBackend::Html => directive::Scope::HtmlDocCk,
            DocBackend::Json => directive::Scope::JsonDocCk,
        },
    }
}

pub(crate) enum Operation {
    Compile { mode: CompileMode, run: Run, options: CompileOptions },
    Document { mode: DocMode, open: Open, options: DocOptions },
}

#[derive(Clone, Copy)]
pub(crate) enum CompileMode {
    Default,
    DirectiveDriven(directive::Flavor, Test),
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
    DirectiveDriven(directive::Flavor, Test),
}

#[derive(Clone, Copy)]
pub(crate) enum Open {
    Yes,
    No,
}

#[derive(Clone, Copy)]
pub(crate) enum Test {
    #[allow(dead_code)] // FIXME
    Yes(Bless),
    No,
}

#[derive(Clone, Copy)]
pub(crate) enum Bless {
    Yes,
    No,
}
