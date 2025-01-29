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
    data::{self, CrateName, CrateNameRef, DocBackend, Identity},
    diagnostic::{Paint, Painter, debug},
    source::Spanned,
    utility::default,
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

type Crate<'a> = data::Crate<'a, Option<Edition<'a>>>;

pub(crate) fn perform(
    engine: Engine<'_>,
    crate_: Crate<'_>,
    extern_crates: &[ExternCrate<'_>],
    flags: Flags<'_>,
    imply_unstable: ImplyUnstableOptions,
) -> io::Result<()> {
    let mut cmd = Command::new(engine.name());
    let uses_unstable_options = configure_basic(&mut cmd, engine, crate_, flags);
    configure_extra(&mut cmd, engine, extern_crates, flags);
    if let ImplyUnstableOptions::Yes = imply_unstable
        && let UsesUnstableOptions::Yes = uses_unstable_options
    {
        cmd.arg("-Zunstable-options");
    }
    execute(cmd, flags.debug)
}

pub(crate) fn query_crate_name(crate_: Crate<'_>, flags: Flags<'_>) -> io::Result<String> {
    let mut cmd = Command::new(Engine::Rustc.name());

    configure_basic(&mut cmd, Engine::Rustc, crate_, flags);

    cmd.arg("--print=crate-name");

    match gate(|p| render(&cmd, p), flags.debug) {
        ControlFlow::Continue(()) => {}
        ControlFlow::Break(()) => return Ok("UNKNOWN_DUE_TO_DRY_RUN".into()),
    }

    // FIXME: Double check that `path`=`-` (STDIN) properly works with `output()` once we support that ourselves.
    let output = cmd.output()?;
    output.status.exit_ok().map_err(io::Error::other)?;
    _ = output.stderr;

    // FIXME: Don't unwrap or return an io::Error, provide a proper bug() instead.
    let mut output = String::from_utf8(output.stdout).unwrap();
    output.truncate(output.trim_end().len());

    Ok(output)
}

fn configure_basic(
    cmd: &mut Command,
    engine: Engine<'_>,
    crate_: Crate<'_>,
    flags: Flags<'_>,
) -> UsesUnstableOptions {
    let mut uses_unstable_options = UsesUnstableOptions::No;

    // Must come first!
    // FIXME: Consider only setting the (rustup) toolchain if the env var `RUSTUP_HOME` exists.
    //        And emitting a warning further up the stack of course.
    if let Some(toolchain) = flags.toolchain {
        cmd.arg(toolchain);
    }

    cmd.arg(crate_.path);
    // FIXME: Get rid of the "fiducial check" (ideally `crate_name` would be an `Option<_>` instead).
    if !CrateName::adjust_and_parse_file_path(crate_.path)
        .is_ok_and(|fiducial_crate_name| crate_.name == fiducial_crate_name.as_ref())
    {
        cmd.arg("--crate-name");
        cmd.arg(crate_.name.as_str());
    }

    // FIXME: get rid of check against default?
    if crate_.type_ != default() {
        cmd.arg("--crate-type");
        cmd.arg(crate_.type_.to_str());
    }

    // Regarding crate name querying, the edition is vital. After all,
    // rustc needs to parse the crate root to find `#![crate_name]`.
    if let Some(edition) = crate_.edition {
        cmd.arg("--edition");

        let edition = match edition {
            Edition::Parsed(edition) => {
                if !edition.is_stable() {
                    uses_unstable_options.set();
                }
                edition.to_str()
            }
            Edition::Raw(edition) => edition,
        };

        cmd.arg(edition);
    }

    // Regarding crate name querying, let's better honor this flag
    // since it may significantly affect rustc's behavior.
    if let Some(identity) = flags.build.identity {
        cmd.env("RUSTC_BOOTSTRAP", match identity {
            Identity::True => "0",
            Identity::Stable => "-1",
            Identity::Nightly => "1",
        });
    }

    configure_verbatim_data(cmd, flags.verbatim);

    engine.configure(cmd, &mut uses_unstable_options);

    if let Some(flags) = engine.env_flags() {
        cmd.args(flags);
    }

    uses_unstable_options
}

// FIXME: Explainer: This is stuff that isn't necessary for query_crate_name
fn configure_extra(
    cmd: &mut Command,
    engine: Engine<'_>,
    extern_crates: &[ExternCrate<'_>],
    flags: Flags<'_>,
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
        for cfg in &flags.build.cfgs {
            cmd.arg("--cfg");
            cmd.arg(cfg);
        }
        // FIXME: This shouldn't be done here.
        for feature in &flags.build.cargo_features {
            // FIXME: Warn on conflicts with `cfgs` from `cmd.arguments.cfgs`.
            // FIXME: collapse
            cmd.arg("--cfg");
            cmd.arg(format!("feature=\"{feature}\""));
        }
        // FIXME: This shouldn't be done here.
        if let Some(revision) = &flags.build.revision {
            cmd.arg("--cfg");
            cmd.arg(revision);
        }
    }

    for feature in &flags.build.rustc_features {
        cmd.arg(format!("-Zcrate-attr=feature({feature})"));
    }

    if flags.build.cap_lints {
        cmd.arg("--cap-lints=warn");
    }

    if flags.build.next_solver {
        cmd.arg("-Znext-solver");
    }

    if flags.build.rustc_verbose_internals {
        cmd.arg("-Zverbose-internals");
    }

    if flags.build.no_backtrace {
        cmd.env("RUST_BACKTRACE", "0");
    }

    // The logging output would just get thrown away.
    if let Some(filter) = &flags.build.log {
        cmd.env(engine.logging_env_var(), filter);
    }
}

fn configure_verbatim_data(cmd: &mut process::Command, verbatim: VerbatimData<'_>) {
    for (key, value) in verbatim.variables {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    // FIXME: This comment is out of context now
    // Regardin crate name querying,...
    // It's vital that we pass through verbatim arguments when querying the crate name as they might
    // contain impactful flags like `--crate-name …`, `-Zcrate-attr=crate_name(…)`, or `--edition …`.
    cmd.args(verbatim.arguments);
}

pub(crate) fn run(
    program: impl AsRef<OsStr>,
    verbatim: VerbatimData<'_>,
    flags: &DebugFlags,
) -> io::Result<()> {
    let mut cmd = Command::new(program);
    configure_verbatim_data(&mut cmd, verbatim);
    execute(cmd, flags)
}

pub(crate) fn open(path: &Path, flags: &DebugFlags) -> io::Result<()> {
    let message = |p: &mut Painter| {
        p.with(palette::COMMAND.on_default().bold(), |p| write!(p, "⟨open⟩ "))?;
        p.with(AnsiColor::Green, |p| write!(p, "{}", path.display()))
    };

    match gate(message, flags) {
        ControlFlow::Continue(()) => open::that(path),
        ControlFlow::Break(()) => Ok(()),
    }
}

#[must_use]
fn gate(message: impl Paint, flags: &DebugFlags) -> ControlFlow<()> {
    if flags.verbose {
        let verb = if !flags.dry_run { "running" } else { "skipping" };
        debug(|p| {
            write!(p, "{verb} ")?;
            message(p)
        })
        .finish();
    }

    match flags.dry_run {
        true => ControlFlow::Break(()),
        false => ControlFlow::Continue(()),
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Engine<'a> {
    Rustc,
    Rustdoc(&'a DocFlags),
}

impl Engine<'_> {
    const fn name<'a>(self) -> &'a str {
        match self {
            Self::Rustc => "rustc",
            Self::Rustdoc(_) => "rustdoc",
        }
    }

    const fn logging_env_var<'a>(self) -> &'a str {
        match self {
            Self::Rustc => "RUSTC_LOG",
            Self::Rustdoc(_) => "RUSTDOC_LOG",
        }
    }

    fn env_flags<'a>(self) -> Option<&'a [String]> {
        match self {
            Self::Rustc => environment::rustc_flags(),
            Self::Rustdoc(_) => environment::rustdoc_flags(),
        }
    }

    fn configure(self, cmd: &mut Command, uses_unstable_options: &mut UsesUnstableOptions) {
        match self {
            Self::Rustc => {}
            Self::Rustdoc(doc_flags) => {
                if let DocBackend::Json = doc_flags.backend {
                    cmd.arg("--output-format=json");
                    uses_unstable_options.set();
                }

                if let Some(crate_version) = &doc_flags.crate_version {
                    cmd.arg("--crate-version");
                    cmd.arg(crate_version);
                }

                if doc_flags.private {
                    cmd.arg("--document-private-items");
                }

                if doc_flags.hidden {
                    cmd.arg("--document-hidden-items");
                    uses_unstable_options.set();
                }

                if doc_flags.layout {
                    cmd.arg("--show-type-layout");
                    uses_unstable_options.set();
                }

                if doc_flags.link_to_definition {
                    cmd.arg("--generate-link-to-definition");
                    uses_unstable_options.set();
                }

                if doc_flags.normalize {
                    cmd.arg("-Znormalize-docs");
                }

                cmd.arg("--default-theme");
                cmd.arg(&doc_flags.theme);
            }
        }
    }
}

fn execute(mut cmd: process::Command, flags: &DebugFlags) -> io::Result<()> {
    match gate(|p| render(&cmd, p), flags) {
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

#[allow(clippy::struct_excessive_bools)] // not worth to address
pub(crate) struct DocFlags {
    pub(crate) backend: DocBackend,
    pub(crate) crate_version: Option<String>,
    pub(crate) private: bool,
    pub(crate) hidden: bool,
    pub(crate) layout: bool,
    pub(crate) link_to_definition: bool,
    pub(crate) normalize: bool,
    pub(crate) theme: String,
}

/// Flags that get passed to `rustc` and `rustdoc` in a lowered form.
#[allow(clippy::struct_excessive_bools)] // not worth to address
pub(crate) struct BuildFlags {
    pub(crate) cfgs: Vec<String>,
    pub(crate) revision: Option<String>,
    // FIXME: This shouldn't be here:
    pub(crate) cargo_features: Vec<String>,
    pub(crate) rustc_features: Vec<String>,
    pub(crate) cap_lints: bool,
    pub(crate) rustc_verbose_internals: bool,
    pub(crate) next_solver: bool,
    pub(crate) identity: Option<Identity>,
    pub(crate) log: Option<String>,
    pub(crate) no_backtrace: bool,
}

pub(crate) struct DebugFlags {
    pub(crate) verbose: bool,
    pub(crate) dry_run: bool,
}

// FIXME: This type leads to such awkward code; consider reworking it.
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum ExternCrate<'src> {
    Unnamed { path: Spanned<&'src str> },
    Named { name: CrateNameRef<'src>, path: Option<Spanned<Cow<'src, str>>> },
}

// FIXME: Name "flags" doesn't quite fit.
// FIXME: Should we / can we move this into `cli` somehow?
#[derive(Clone, Copy)]
pub(crate) struct Flags<'a> {
    pub(crate) toolchain: Option<&'a OsStr>,
    pub(crate) build: &'a BuildFlags,
    pub(crate) verbatim: VerbatimData<'a>,
    pub(crate) debug: &'a DebugFlags,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct VerbatimData<'a> {
    /// Program arguments to be passed verbatim.
    pub(crate) arguments: &'a [&'a str],
    /// Environment variables to be passed verbatim.
    pub(crate) variables: &'a [(&'a str, Option<&'a str>)],
}

/// Program arguments and environment variables to be passed verbatim.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct VerbatimDataBuf<'a> {
    /// Program arguments to be passed verbatim.
    pub(crate) arguments: Vec<&'a str>,
    /// Environment variables to be passed verbatim.
    pub(crate) variables: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> VerbatimDataBuf<'a> {
    pub(crate) fn extend(&mut self, other: VerbatimData<'a>) {
        self.arguments.extend_from_slice(other.arguments);
        self.variables.extend_from_slice(other.variables);
    }

    pub(crate) fn as_ref(&self) -> VerbatimData<'_> {
        VerbatimData { arguments: &self.arguments, variables: &self.variables }
    }
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

#[derive(Clone, Copy)]
pub(crate) enum Edition<'a> {
    Parsed(data::Edition),
    Raw(&'a str),
}
