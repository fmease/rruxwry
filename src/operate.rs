//! High-level build operations.
//!
//! The low-level build routines are defined in [`crate::build`].

// FIXME: Add explainer about why we use `--print=crate-name` over `-o` (crate type nuisance; rustdoc no likey).

// FIXME: Create test for `//@ compile-flags: --extern name` + aux-build
// FIXME: Create test for `//@ compile-flags: --test`.

use crate::{
    build::{
        self, CompileOptions, DocOptions, Engine, EngineOptions, ImplyUnstableOptions, Options,
        QueryEngineVersionError, VerbatimOptions,
    },
    context::Context,
    data::{Crate, CrateName, CrateType, DocBackend, Edition, ExtEdition},
    diagnostic::{error, fmt, warn},
    directive,
    error::Result,
    source::{SourcePath, Spanned},
    utility::{OsStrExt as _, default, paint::Painter},
};
use anstyle::AnsiColor;
use std::{
    ascii::Char,
    borrow::Cow,
    cell::LazyCell,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

// FIXME: `-@V` may fail to report fake identities as set via `//@ rustc-env: RUST_BOOTSTRAP=…`.
//        Should we consider that a bug or "out of scope"? We could theoretically gather directives
//        under `-@V` to obtain this piece of information.

pub(crate) fn perform(
    op: Operation,
    krate: Crate<'_, ExtEdition<'_>>,
    opts: Options<'_>,
    cx: Context<'_>,
) -> Result<()> {
    let paint_err = |error: QueryEngineVersionError, p: &mut Painter<_>| {
        p.with(AnsiColor::Red, |p| write!(p, "{{ {} }}", error.short_desc()))
    };

    match op {
        Operation::Compile { mode, run, options: c_opts } => {
            compile(mode, run, krate, opts, c_opts, cx)
        }
        Operation::QueryRustcVersion => {
            // Don't create a fresh painter that's expensive! Use the one from the Context!
            let stdout = io::stdout().lock();
            let colorize = anstream::AutoStream::choice(&stdout) != anstream::ColorChoice::Never;
            let mut p = Painter::new(io::BufWriter::new(stdout), colorize);

            write!(p, "rustc: ")?;
            match Engine::Rustc.version(cx) {
                Ok(version) => version.paint(build::probe_identity(&opts), &mut p),
                Err(error) => paint_err(error, &mut p),
            }?;

            writeln!(p)?;
            Ok(())
        }
        Operation::Document { mode, open, options: d_opts } => {
            document(mode, open, krate, opts, d_opts, cx)
        }
        Operation::QueryRustdocVersion => {
            // Don't create a fresh painter that's expensive! Use the one from the Context!
            let stdout = io::stdout().lock();
            let colorize = anstream::AutoStream::choice(&stdout) != anstream::ColorChoice::Never;
            let mut p = Painter::new(io::BufWriter::new(stdout), colorize);

            write!(p, "rustdoc: ")?;
            match Engine::Rustdoc.version(cx) {
                Ok(version) => version.paint(build::probe_identity(&opts), &mut p),
                Err(error) => paint_err(error, &mut p),
            }?;

            writeln!(p)?;
            write!(p, "  rustc: ")?;
            match Engine::Rustc.version(cx) {
                Ok(version) => version.paint(build::probe_identity(&opts), &mut p),
                Err(error) => paint_err(error, &mut p),
            }?;

            writeln!(p)?;
            Ok(())
        }
    }
}

fn compile<'a>(
    mode: CompileMode,
    run: Run,
    krate: Crate<'a, ExtEdition<'a>>,
    opts: Options<'a>,
    c_opts: CompileOptions,
    cx: Context<'a>,
) -> Result {
    let mut e_opts = EngineOptions::Rustc(c_opts);
    let (krate, opts, run_v_opts) = match mode {
        CompileMode::Default => {
            let typ = krate.typ.or_else(|| matches!(run, Run::No).then_some(CrateType::LIB));
            let krate = Crate { typ, ..krate };
            let (krate, opts) = build_default(&e_opts, krate, opts, cx)?;
            (krate, opts, default())
        }
        CompileMode::DirectiveDriven(dir_opts) => {
            build_directive_driven(&mut e_opts, krate, dir_opts, opts, cx)?
        }
    };
    match run {
        Run::Yes => self::run(krate, &opts, &run_v_opts, cx),
        Run::No => Ok(()),
    }
}

fn run(
    krate: Crate<'_>,
    opts: &Options<'_>,
    run_v_opts: &VerbatimOptions<'_>,
    cx: Context<'_>,
) -> Result {
    // FIXME: Explain why we need to query the crate name.
    let crate_name = build::query_crate_name(krate, opts, cx).map_err(|error| {
        // FIXME: Actually create a 'parent' error diagnostic with a message akin to
        //        "failed to run the built binary (requested …)" and smh.
        //        'tuck' the QueryCrateNameError below it (i.e., more indented).
        error.emit()
    })?;

    let path = if let Some(path) = krate.path
        && let Ok(path) = CrateName::prepare_source_path(path)
        && CrateName::parse_relaxed(path).is_ok_and(|name| name.as_ref() == crate_name.as_ref())
    {
        path
    } else {
        crate_name.as_str()
    };

    let mut path = PathBuf::from_iter([".", path]);
    path.set_extension(std::env::consts::EXE_EXTENSION);

    build::run(&path, run_v_opts, cx)
        .map_err(|error| {
            self::error(fmt!("failed to run the built binary `{}`", path.display()))
                .note(fmt!("{error}"))
                .done()
        })?
        .map_err(|error| {
            self::error(fmt!("process for `{}` exited unsuccessfully", path.display()))
                .note(fmt!("{}", error.into_status()))
                .done()
        })?;
    Ok(())
}

fn document<'a>(
    mode: DocMode,
    open: Open,
    krate: Crate<'a, ExtEdition<'a>>,
    opts: Options<'a>,
    d_opts: DocOptions<'a>,
    cx: Context<'a>,
) -> Result<()> {
    let (krate, opts) = match mode {
        DocMode::Default => build_default(&EngineOptions::Rustdoc(d_opts), krate, opts, cx)?,
        DocMode::CrossCrate => return document_cross_crate(krate, opts, d_opts, open, cx),
        DocMode::DirectiveDriven(dir_opts) => {
            let (krate, opts, _) = build_directive_driven(
                &mut EngineOptions::Rustdoc(d_opts),
                krate,
                dir_opts,
                opts,
                cx,
            )?;
            (krate, opts)
        }
    };
    match open {
        Open::Yes => self::open(krate, &opts, cx),
        Open::No => Ok(()),
    }
}

fn open(krate: Crate<'_>, opts: &Options<'_>, cx: Context<'_>) -> Result<()> {
    // FIXME: Explain why we need to query the crate name.
    let crate_name = build::query_crate_name(krate, opts, cx).map_err(|error| {
        // FIXME: Actually create a 'parent' error diagnostic with a message akin to
        //        "failed to open the generated docs (requested …)" and smh.
        //        'tuck' the QueryCrateNameError below it (i.e., more indented).
        error.emit()
    })?;

    let path = format!("./doc/{crate_name}/index.html");

    build::open(Path::new(&path), cx).map_err(|error| {
        self::error(fmt!("failed to open the generated docs in a browser"))
            .note(fmt!("{error}"))
            .done()
    })?;
    Ok(())
}

fn document_cross_crate(
    krate: Crate<'_, ExtEdition<'_>>,
    mut opts: Options<'_>,
    d_opts: DocOptions<'_>,
    open: Open,
    cx: Context<'_>,
) -> Result<()> {
    let path = krate.path.ok_or_else(|| {
        error(fmt!(
            "the `PATH` argument was not provided but it's required under `-X`, `--cross-crate`"
        ))
        .done()
    })?;

    let krate = Crate { typ: krate.typ.or(Some(CrateType::LIB)), ..krate };
    // FIXME: The clone is awful!
    let (krate, _) = build_default(&EngineOptions::Rustc(default()), krate, opts.clone(), cx)?;

    // FIXME: This `unwrap` is obviously reachable (e.g., on `rrc '%$?'`)
    let crate_name: CrateName<Cow<'_, _>> = krate
        .name
        .map_or_else(|| CrateName::parse_source_file_relaxed(path).unwrap().into(), Into::into);

    let root_crate_name = CrateName::new_unchecked(format!("u_{crate_name}"));
    let root_crate_path = match path {
        SourcePath::Regular(path) => path,
        SourcePath::Stdin => Path::new(""),
    }
    .with_file_name(root_crate_name.as_str())
    .with_extension("rs");

    if !root_crate_path.exists() {
        // While we could omit the `extern crate` declaration in `edition >= Edition::Edition2018`,
        // we would need to recreate the file on each rerun if the edition was 2015 instead of
        // skipping that step since we wouldn't know whether the existing file if applicable was
        // created for a newer edition or not.
        //
        // Moreover keeping this file enables users to modify it after the fact without the fear
        // of it getting overwritten during the next run.
        std::fs::write(
            &root_crate_path,
            format!("extern crate {crate_name}; pub use {crate_name}::*;\n"),
        )?;
    }

    // FIXME: Don't to_owned, extern_crates should be a Cow
    opts.b_opts.extern_crates.push(crate_name.as_str().to_owned());

    let krate = Crate {
        path: Some(SourcePath::Regular(&root_crate_path)),
        name: Some(root_crate_name.as_ref()),
        typ: None,
        ..krate
    };

    build::perform(&EngineOptions::Rustdoc(d_opts), krate, &opts, ImplyUnstableOptions::Yes, cx)?;

    // FIXME: Move this out of this function into the caller `document` to further simplify things
    match open {
        Open::Yes => self::open(krate, &opts, cx),
        Open::No => Ok(()),
    }
}

fn build_default<'a>(
    e_opts: &EngineOptions<'_>,
    krate: Crate<'a, ExtEdition<'a>>,
    mut opts: Options<'a>,
    cx: Context<'_>,
) -> Result<(Crate<'a>, Options<'a>)> {
    // FIXME: Only querying the lastest stable edition of this (the primary) engine
    //        might not be correct for engine==rustdoc since Op::Doc may invoke
    //        rustc too and since here are nightly (& stable?) releases where rustc
    //        and rustdoc differ wrt. to their stable edition IINM.
    //
    //        Figure out if there are such releases and if so how to best address it.
    let edition = krate.edition.unwrap_or(ExtEdition::LatestStable).resolve(e_opts.engine(), cx);
    let krate = Crate { edition, ..krate };
    populate_extern_prelude(krate.typ, &mut opts.b_opts.extern_crates);
    build::perform(e_opts, krate, &opts, ImplyUnstableOptions::Yes, cx)?;
    Ok((krate, opts))
}

fn build_directive_driven<'a>(
    e_opts: &mut EngineOptions<'a>,
    krate: Crate<'a, ExtEdition<'a>>,
    mut dir_opts: DirectiveOptions,
    mut opts: Options<'a>,
    cx: Context<'a>,
) -> Result<(Crate<'a>, Options<'a>, VerbatimOptions<'a>)> {
    let path = krate.path.ok_or_else(|| {
        error(fmt!(
            "the `PATH` argument was not provided but it's required under `-@`, `--directives`"
        ))
        .done()
    })?;

    let (path, revision_from_path) = if let SourcePath::Regular(path) = path
        && let Some((path, revision)) = path.as_os_str().rsplit_once(Char::NumberSign)
    {
        let Some(revision) = revision.to_str() else {
            return Err(error(fmt!(
                "active revision suffix `{}` is not valid UTF-8",
                revision.display()
            ))
            .done()
            .into());
        };
        (SourcePath::Regular(Path::new(path)), Some(revision))
    } else {
        (path, None)
    };

    match (revision_from_path, &dir_opts.revision) {
        (Some(rev0), Some(rev1)) if rev0 == rev1 => {
            warn(fmt!("the active revision `{rev0}` was passed twice"))
                .note(fmt!("once as a path suffix, once via a flag"))
                .done();
        }
        (Some(rev0), Some(rev1)) => {
            return Err(error(fmt!("two conflicting active revisions were passed"))
                .note(fmt!("path suffix `{rev0}` and flag argument `{rev1}` do not match"))
                .done()
                .into());
        }
        (Some(rev), None) => dir_opts.revision = Some(rev.to_owned()),
        (None, _) => {}
    }

    let directives = directive::gather(
        Spanned::sham(path),
        scope(e_opts),
        directive::Role::Principal,
        dir_opts.flavor,
        dir_opts.revision.as_deref(),
        cx,
    )?;

    let aux_base_path = LazyCell::new(|| {
        match path {
            // FIXME: unwrap
            SourcePath::Regular(path) => path.parent().unwrap(),
            SourcePath::Stdin => Path::new(""),
        }
        .join("auxiliary")
    });
    let mut extern_crates = Vec::new();

    directives.auxes.iter().try_for_each(|aux| {
        compile_auxiliary(
            aux,
            &aux_base_path,
            e_opts,
            dir_opts.clone(),
            // FIXME: This awful!
            opts.clone(),
            directives.build_aux_docs,
            cx,
            &mut extern_crates,
        )
    })?;

    if let Some(revision) = dir_opts.revision {
        opts.b_opts.cfgs.push(revision);
    }

    opts.b_opts.extern_crates.append(&mut extern_crates);

    // Note that if `prefer_dylib` was `Yes` compiletest would now add `-Cprefer-dynamic`
    // if this was `Engine::Rustc`. We might need to do that to at some point, tho I'm
    // honestly not sure about all the consequences. Also note that we currently use a
    // crate type of lib instead of dylib for auxiliaries by default.
    _ = directives.prefer_dylib;

    let edition = match krate.edition {
        // If the resolution of the CLI edition fails, we *don't*
        // want to fall back to the directive edition.
        // FIXME: Passing this (the primary) engine to resolve might not be correct
        //        for engine==rustdoc see comment above the other invocation of
        //        `resolve` in this module.
        Some(edition) => edition.resolve(e_opts.engine(), cx),
        None => directives.edition.map(|edition| Edition::Unknown(edition.bare)),
    };

    let krate = Crate { path: Some(path), edition, ..krate };

    opts.v_opts.extend(directives.v_opts);
    match e_opts {
        EngineOptions::Rustc(..) => {} // rustc-exclusive (verbatim) flags is not a thing.
        EngineOptions::Rustdoc(d_opts) => d_opts.v_opts.extend(directives.v_d_opts),
    }

    build::perform(e_opts, krate, &opts, ImplyUnstableOptions::No, cx)?;
    Ok((krate, opts, directives.run_v_opts))
}

// FIXME: Support nested auxiliaries!
// FIXME: Detect and reject circular/cyclic auxiliaries.
fn compile_auxiliary<'a>(
    &directive::Auxiliary { ref prefix, path, typ }: &directive::Auxiliary<'a>,
    base_path: &Path,
    e_opts: &EngineOptions<'_>,
    dir_opts: DirectiveOptions,
    // FIXME: Do we actually want to pass along *all* of these opts?
    //        Arguably some of them belong to the root crate only (e.g. crate name).
    //        On top of that, the status quo is inconsistent because
    //        we don't honor the edition (which is also just an "option").
    //        Some options should however be inherited: toolchain, cfgs, rev,
    //        debug. Should subset vs. all be a CLI option?
    mut opts: Options<'a>,
    doc: bool,
    cx: Context<'a>,
    parent_extern_crates: &mut Vec<String>,
) -> Result<()> {
    let path = path.map(|path| base_path.join(path));

    let directives = directive::gather(
        path.as_deref().map(SourcePath::Regular),
        scope(e_opts),
        directive::Role::Auxiliary,
        dir_opts.flavor,
        dir_opts.revision.as_deref(),
        cx,
    )?;

    if let Some(revision) = dir_opts.revision {
        opts.b_opts.cfgs.push(revision);
    }

    let directive::InstantiatedDirectives {
        edition,
        v_opts,
        prefer_dylib,
        // FIXME
        build_aux_docs: _,
        // FIXME
        auxes: _,
        v_d_opts: _,
        run_v_opts: _,
    } = directives;

    opts.v_opts.extend(v_opts);

    // Note that if `prefer_dylib` was `Yes` compiletest would now add `-Cprefer-dynamic`
    // if this was `Engine::Rustc`. Please see note in `build_directive_driven`.

    let krate = Crate {
        path: Some(SourcePath::Regular(&path.bare)),
        name: None,
        typ: prefer_dylib.apply(typ),
        edition: edition.map(|edition| Edition::Unknown(edition.bare)),
    };

    // FIXME: Also register library search paths properly
    if let Some(prefix) = prefix {
        parent_extern_crates.push(prefix.to_string());
    }

    populate_extern_prelude(krate.typ, &mut opts.b_opts.extern_crates);

    build::perform(
        match e_opts {
            // FIXME: Does this actually work as expected wrt. to check-only?
            //        Does this lead to all crates in the dependency graph to
            //        get checked-only and everything working out (linking correctly etc)?
            //        I suspect is doesn't because we need to s%/rlib/rmeta/
            EngineOptions::Rustc(..) => e_opts,
            EngineOptions::Rustdoc(_) => const { &EngineOptions::Rustc(CompileOptions::default()) },
        },
        krate,
        &opts,
        ImplyUnstableOptions::No,
        cx,
    )?;

    if doc && let EngineOptions::Rustdoc(d_opts) = e_opts {
        build::perform(
            // FIXME: Do we actually want to forward these doc opts from the parent crate??
            &EngineOptions::Rustdoc(d_opts.clone()),
            krate,
            &opts,
            ImplyUnstableOptions::No,
            cx,
        )?;
    }

    Ok(())
}

fn scope(e_opts: &EngineOptions<'_>) -> directive::Scope {
    match e_opts {
        EngineOptions::Rustc(..) => directive::Scope::Base,
        // FIXME: Do we actually want to treat !`-j` as `rustdoc/` (Scope::HtmlDocCk)
        //        instead of `rustdoc-ui/` ("Scope::Rustdoc")
        EngineOptions::Rustdoc(d_opts) => match d_opts.backend {
            DocBackend::Html => directive::Scope::HtmlDocCk,
            DocBackend::Json => directive::Scope::JsonDocCk,
        },
    }
}

impl directive::PreferDylib {
    fn apply(self, typ: Option<CrateType>) -> Option<CrateType> {
        match (self, typ) {
            (_, typ @ Some(_)) => typ,
            // Compiletest defaults to `dylib` unless the target architecture
            // doesn't support dynamic linking in which case it also uses `lib`.
            // Since we don't have any "infrastructure" in place for checking
            // target architectures, let's fall back to the "safer" option.
            (Self::Yes, None) => Some(CrateType::LIB),
            (Self::No, None) => None,
        }
    }
}

fn populate_extern_prelude(typ: Option<CrateType>, extern_crates: &mut Vec<String>) {
    match typ {
        // For convenience and just like Cargo we add `proc_macro` to the external prelude.
        // FIXME: Don't to_string, use Cow
        Some(CrateType::PROC_MACRO) => extern_crates.push("proc_macro".to_string()),
        _ => {}
    }
}

pub(crate) enum Operation {
    Compile { mode: CompileMode, run: Run, options: CompileOptions },
    QueryRustcVersion,
    Document { mode: DocMode, open: Open, options: DocOptions<'static> },
    QueryRustdocVersion,
}

pub(crate) enum CompileMode {
    Default,
    DirectiveDriven(DirectiveOptions),
}

pub(crate) enum DocMode {
    Default,
    CrossCrate,
    DirectiveDriven(DirectiveOptions),
}

#[derive(Clone)]
pub(crate) struct DirectiveOptions {
    pub(crate) flavor: directive::Flavor,
    pub(crate) revision: Option<String>,
    #[expect(dead_code)] // FIXME
    pub(crate) test: Test,
}

#[derive(Clone, Copy)]
pub(crate) enum Test {
    #[expect(dead_code)] // FIXME
    Yes(Bless),
    No,
}

#[derive(Clone, Copy)]
pub(crate) enum Bless {
    Yes,
    No,
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
