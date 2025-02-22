//! Low-level build routines.
//!
//! The high-level build operations are defined in [`crate::operate`].

// FIXME: Stop doing that, it's unnecessary and wasted effort:
// Note that we try to avoid generating unnecessary flags where possible even if that means
// doing more work on our side. The main motivation for this is being able to just copy/paste
// the commands printed by `--verbose` for use in GitHub discussions without requiring any
// manual minimization.
// FIXME: Also mention to reduce conflicts with compile flags passed via `compiletest`
//        as well as those passed via the `RUST{,DOC}FLAGS` env vars.

use crate::{
    data::{Crate, CrateName, CrateType, DocBackend, Identity},
    diagnostic::{Paint, Painter, debug},
    source::Spanned,
};
use anstyle::{AnsiColor, Effects};
use std::{
    borrow::Cow,
    ffi::OsStr,
    io::{self, Write},
    ops::ControlFlow,
    path::Path,
    process::{self, Command},
};

mod environment;

pub(crate) fn perform(
    engine: &Engine<'_>,
    krate: Crate<'_>,
    extern_crates: &[ExternCrate<'_>],
    opts: &Options<'_>,
    imply_u_opts: ImplyUnstableOptions,
) -> io::Result<()> {
    let mut cmd = Command::new(engine.name());
    let u_opts = configure_basic(&mut cmd, engine, krate, opts);
    configure_extra(&mut cmd, engine, extern_crates, opts);
    if let ImplyUnstableOptions::Yes = imply_u_opts
        && let UsesUnstableOptions::Yes = u_opts
    {
        cmd.arg("-Zunstable-options");
    }
    execute(cmd, opts.dbg_opts)
}

pub(crate) fn query_crate_name(krate: Crate<'_>, opts: &Options<'_>) -> io::Result<String> {
    let engine = Engine::Rustc(CompileOptions { check_only: false });

    let mut cmd = Command::new(engine.name());

    configure_basic(&mut cmd, &engine, krate, opts);

    cmd.arg("--print=crate-name");

    match gate(|p| render(&cmd, p), opts.dbg_opts) {
        ControlFlow::Continue(()) => {}
        ControlFlow::Break(()) => return Ok("UNKNOWN_DUE_TO_DRY_RUN".into()),
    }

    // FIXME: Double check that `path`=`-` (STDIN) properly works with `output()` once we support that ourselves.
    let output = cmd.output()?;
    _ = output.stderr;

    // If we trigger this we likely passed incorrect flags to the rustc invocation.
    // FIXME: Unfortunately, this can actually be triggered in practice under `d -@`
    // since we pass along verbatim flags as obtained by `compile-flags` directives
    // which may "erroneously" contain rustdoc-specific flags. See also
    // <https://github.com/rust-lang/rust/issues/137442>.
    // If the r-l/r doesn't go anywhere provide a mechanism(s) (via a flag) to
    // somehow remedy this situation (e.g., filtering flags or skipping given
    // directives).
    assert!(output.status.success(), "failed to properly query rustc about the crate name");

    // FIXME: Don't unwrap or return an io::Error, provide a proper bug() instead.
    let mut output = String::from_utf8(output.stdout).unwrap();
    output.truncate(output.trim_end().len());

    Ok(output)
}

fn configure_basic(
    cmd: &mut Command,
    engine: &Engine<'_>,
    krate: Crate<'_>,
    opts: &Options<'_>,
) -> UsesUnstableOptions {
    let mut u_opts = UsesUnstableOptions::No;

    // Must come first!
    // FIXME: Consider only setting the (rustup) toolchain if the env var `RUSTUP_HOME` exists.
    //        And emitting a warning further up the stack of course.
    if let Some(toolchain) = opts.toolchain {
        cmd.arg(toolchain);
    }

    cmd.arg(krate.path);

    if let Some(name) = krate.name {
        cmd.arg("--crate-name");
        cmd.arg(name.as_str());
    }

    if let Some(CrateType(typ)) = krate.typ {
        cmd.arg("--crate-type");
        cmd.arg(typ);
    }

    // Regarding crate name querying, the edition is vital. After all,
    // rustc needs to parse the crate root to find `#![crate_name]`.
    if let Some(edition) = krate.edition {
        if !edition.is_stable() {
            u_opts.set();
        }

        cmd.arg("--edition");
        cmd.arg(edition.to_str());
    }

    // Regarding crate name querying, let's better honor this option
    // since it may significantly affect rustc's behavior.
    if let Some(identity) = opts.b_opts.identity {
        cmd.env("RUSTC_BOOTSTRAP", match identity {
            Identity::True => "0",
            Identity::Stable => "-1",
            Identity::Nightly => "1",
        });
    }

    configure_v_opts(cmd, &opts.v_opts);
    configure_engine_specific(cmd, engine, &mut u_opts);

    if let Some(opts) = engine.env_opts() {
        cmd.args(opts);
    }

    u_opts
}

// FIXME: Explainer: This is stuff that isn't necessary for query_crate_name
fn configure_extra(
    cmd: &mut Command,
    engine: &Engine<'_>,
    extern_crates: &[ExternCrate<'_>],
    opts: &Options<'_>,
) {
    // The crate name can't depend on any dependency crates, it's fine to skip this.
    // The opposite used to be the case actually prior to rust-lang/rust#117584.
    // E.g., via `#![crate_name = dependency::generate!()]`.
    // This no longer works and will be a hard error soon (rust-lang/rust#127581).
    // Lastly, it's impossible for a macro to generate inner attributes (rust-lang/rust#66920)
    // and even if that were to change at some point, rustc will never expand macros
    // in order to find `#![crate_name]` (ruled by T-lang).

    // What does `compiletest` do?
    if !extern_crates.is_empty() {
        // FIXME: Does this work with proc macro deps? I think so?
        // FIXME: This is hacky, rework it.
        cmd.arg("-Lcrate=.");
    }

    for extern_crate in extern_crates {
        let ExternCrate::Named { name, path } = extern_crate else {
            continue;
        };

        cmd.arg("--extern");
        match path {
            Some(path) => cmd.arg(format!("{name}={}", path.bare)),
            None => cmd.arg(name.as_str()),
        };
    }

    // The crate name can't depend on any cfgs, it's fine to skip this.
    // In the past this wasn't really the case but since 1.83
    // `#[cfg_attr(…, crate_name = "…")]` is a hard error (if the spec holds).
    // See also rust-lang/rust#91632.
    {
        for cfg in &opts.b_opts.cfgs {
            cmd.arg("--cfg");
            cmd.arg(cfg);
        }
        // FIXME: This shouldn't be done here.
        for feature in &opts.b_opts.cargo_features {
            // FIXME: Warn on conflicts with `cfgs` from `cmd.arguments.cfgs`.
            // FIXME: collapse
            cmd.arg("--cfg");
            cmd.arg(format!("feature=\"{feature}\""));
        }
        // FIXME: This shouldn't be done here.
        if let Some(revision) = &opts.b_opts.revision {
            cmd.arg("--cfg");
            cmd.arg(revision);
        }
    }

    for feature in &opts.b_opts.rustc_features {
        cmd.arg(format!("-Zcrate-attr=feature({feature})"));
    }

    if opts.b_opts.suppress_lints {
        cmd.arg("--cap-lints=allow");
    }

    if opts.b_opts.next_solver {
        cmd.arg("-Znext-solver");
    }

    if opts.b_opts.internals {
        cmd.arg("-Zverbose-internals");
    }

    if opts.b_opts.no_backtrace {
        cmd.env("RUST_BACKTRACE", "0");
    }

    // The logging output would just get thrown away.
    if let Some(filter) = &opts.b_opts.log {
        cmd.env(engine.logging_env_var(), filter);
    }
}

fn configure_v_opts(cmd: &mut process::Command, v_opts: &VerbatimOptions<'_>) {
    for (key, value) in &v_opts.variables {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    // FIXME: This comment is out of context now
    // Regardin crate name querying,...
    // It's vital that we pass through verbatim arguments when querying the crate name as they might
    // contain impactful options like `--crate-name …`, `-Zcrate-attr=crate_name(…)`, or `--edition …`.
    cmd.args(&v_opts.arguments);
}

fn configure_engine_specific(
    cmd: &mut Command,
    engine: &Engine<'_>,
    u_opts: &mut UsesUnstableOptions,
) {
    match engine {
        Engine::Rustc(c_opts) => {
            if c_opts.check_only {
                // FIXME: Should we `-o $null`?
                cmd.arg("--emit=metadata");
            }
        }
        Engine::Rustdoc(d_opts) => {
            if let DocBackend::Json = d_opts.backend {
                cmd.arg("--output-format=json");
                u_opts.set();
            }

            if let Some(crate_version) = &d_opts.crate_version {
                cmd.arg("--crate-version");
                cmd.arg(crate_version);
            }

            if d_opts.private {
                cmd.arg("--document-private-items");
            }

            if d_opts.hidden {
                cmd.arg("--document-hidden-items");
                u_opts.set();
            }

            if d_opts.layout {
                cmd.arg("--show-type-layout");
                u_opts.set();
            }

            if d_opts.link_to_def {
                cmd.arg("--generate-link-to-definition");
                u_opts.set();
            }

            if d_opts.normalize {
                cmd.arg("-Znormalize-docs");
            }

            cmd.arg("--default-theme");
            cmd.arg(&d_opts.theme);

            cmd.args(&d_opts.v_opts.arguments);
        }
    }
}

pub(crate) fn run(
    program: impl AsRef<OsStr>,
    v_opts: &VerbatimOptions<'_>,
    dbg_opts: &DebugOptions,
) -> io::Result<()> {
    let mut cmd = Command::new(program);
    configure_v_opts(&mut cmd, v_opts);
    execute(cmd, dbg_opts)
}

pub(crate) fn open(path: &Path, dbg_opts: &DebugOptions) -> io::Result<()> {
    let message = |p: &mut Painter| {
        p.with(palette::COMMAND.on_default().bold(), |p| write!(p, "⟨open⟩ "))?;
        p.with(AnsiColor::Green, |p| write!(p, "{}", path.display()))
    };

    match gate(message, dbg_opts) {
        ControlFlow::Continue(()) => open::that(path),
        ControlFlow::Break(()) => Ok(()),
    }
}

#[must_use]
fn gate(message: impl Paint, dbg_opts: &DebugOptions) -> ControlFlow<()> {
    if dbg_opts.verbose {
        let verb = if !dbg_opts.dry_run { "running" } else { "skipping" };
        debug(|p| {
            write!(p, "{verb} ")?;
            message(p)
        })
        .finish();
    }

    match dbg_opts.dry_run {
        true => ControlFlow::Break(()),
        false => ControlFlow::Continue(()),
    }
}

pub(crate) enum Engine<'a> {
    Rustc(CompileOptions),
    Rustdoc(DocOptions<'a>),
}

impl Engine<'_> {
    const fn name(&self) -> &'static str {
        match self {
            Self::Rustc(_) => "rustc",
            Self::Rustdoc(_) => "rustdoc",
        }
    }

    const fn logging_env_var(&self) -> &'static str {
        match self {
            Self::Rustc(_) => "RUSTC_LOG",
            Self::Rustdoc(_) => "RUSTDOC_LOG",
        }
    }

    fn env_opts(&self) -> Option<&'static [String]> {
        match self {
            Self::Rustc(_) => environment::rustc_options(),
            Self::Rustdoc(_) => environment::rustdoc_options(),
        }
    }
}

fn execute(mut cmd: process::Command, dbg_opts: &DebugOptions) -> io::Result<()> {
    match gate(|p| render(&cmd, p), dbg_opts) {
        ControlFlow::Continue(()) => cmd.status()?.exit_ok().map_err(io::Error::other),
        ControlFlow::Break(()) => Ok(()),
    }
}

// This is very close to `<process::Command as fmt::Debug>::fmt` but prettier.
// FIXME: This lacks shell escaping!
fn render(cmd: &process::Command, p: &mut Painter) -> io::Result<()> {
    #[allow(irrefutable_let_patterns)]
    if let envs = cmd.get_envs()
        && !envs.is_empty()
    {
        p.set(palette::VARIABLE)?;
        for (key, value) in cmd.get_envs() {
            // FIXME: Print `env -u VAR` for removed vars before
            // added vars just like `Command`'s `Debug` impl.
            let Some(value) = value else { continue };

            p.with(Effects::BOLD, |p| write!(p, "{}", key.display()))?;
            write!(p, "={} ", value.display())?;
        }
        p.unset()?;
    }

    p.with(palette::COMMAND.on_default().bold(), |p| write!(p, "{}", cmd.get_program().display()))?;

    for arg in cmd.get_args() {
        p.with(palette::ARGUMENT, |p| write!(p, " {}", arg.display()))?;
    }

    Ok(())
}

mod palette {
    use anstyle::AnsiColor;

    pub(super) const VARIABLE: AnsiColor = AnsiColor::Yellow;
    pub(super) const COMMAND: AnsiColor = AnsiColor::Magenta;
    pub(super) const ARGUMENT: AnsiColor = AnsiColor::Green;
}

pub(crate) struct CompileOptions {
    pub(crate) check_only: bool,
}

#[allow(clippy::struct_excessive_bools)] // not worth to address
#[derive(Clone)]
pub(crate) struct DocOptions<'a> {
    pub(crate) backend: DocBackend,
    pub(crate) crate_version: Option<String>,
    pub(crate) private: bool,
    pub(crate) hidden: bool,
    pub(crate) layout: bool,
    pub(crate) link_to_def: bool,
    pub(crate) normalize: bool,
    pub(crate) theme: String,
    pub(crate) v_opts: VerbatimOptions<'a, ()>,
}

#[allow(clippy::struct_excessive_bools)] // not worth to address
pub(crate) struct BuildOptions {
    pub(crate) cfgs: Vec<String>,
    pub(crate) revision: Option<String>,
    // FIXME: This shouldn't be here:
    pub(crate) cargo_features: Vec<String>,
    pub(crate) rustc_features: Vec<String>,
    pub(crate) suppress_lints: bool,
    pub(crate) internals: bool,
    pub(crate) next_solver: bool,
    pub(crate) identity: Option<Identity>,
    pub(crate) log: Option<String>,
    pub(crate) no_backtrace: bool,
}

pub(crate) struct DebugOptions {
    pub(crate) verbose: bool,
    pub(crate) dry_run: bool,
}

// FIXME: This type leads to such awkward code; consider reworking it.
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum ExternCrate<'src> {
    Unnamed { path: Spanned<&'src str> },
    Named { name: CrateName<&'src str>, path: Option<Spanned<Cow<'src, str>>> },
}

#[derive(Clone)]
pub(crate) struct Options<'a> {
    pub(crate) toolchain: Option<&'a OsStr>,
    pub(crate) b_opts: &'a BuildOptions,
    pub(crate) v_opts: VerbatimOptions<'a>,
    pub(crate) dbg_opts: &'a DebugOptions,
}

/// Program arguments and environment variables to be passed verbatim.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct VerbatimOptions<'a, V: Append = Vec<(&'a str, Option<&'a str>)>> {
    /// Program arguments to be passed verbatim.
    pub(crate) arguments: Vec<&'a str>,
    /// Environment variables to be passed verbatim.
    pub(crate) variables: V,
}

impl<'a, V: Append> VerbatimOptions<'a, V> {
    pub(crate) fn extend(&mut self, mut other: VerbatimOptions<'a, V>) {
        self.arguments.append(&mut other.arguments);
        self.variables.append(&mut other.variables);
    }
}

pub(crate) trait Append {
    fn append(&mut self, other: &mut Self);
}

impl<T> Append for Vec<T> {
    fn append(&mut self, other: &mut Self) {
        self.append(other);
    }
}

impl Append for () {
    fn append(&mut self, (): &mut Self) {}
}

/// Whether to imply `-Zunstable-options`.
#[derive(Clone, Copy)]
pub(crate) enum ImplyUnstableOptions {
    Yes,
    No,
}

#[derive(Clone, Copy)]
enum UsesUnstableOptions {
    Yes,
    No,
}

impl UsesUnstableOptions {
    fn set(&mut self) {
        *self = Self::Yes;
    }
}
