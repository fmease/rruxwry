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
    context::Context,
    data::{Channel, Crate, CrateName, CrateType, D, DocBackend, Identity, V, Version},
    diagnostic::{self, Diagnostic, EmittedError, Paint, debug, error},
    error::Result,
    fmt,
    source::Spanned,
    utility::default,
};
use anstyle::{AnsiColor, Effects};
use std::{
    borrow::Cow,
    ffi::OsStr,
    io::{self, Write as _},
    ops::ControlFlow,
    path::Path,
    process::{self, Command},
};

mod environment;

pub(crate) fn perform(
    e_opts: &EngineOptions<'_>,
    krate: Crate<'_>,
    extern_crates: &[ExternCrate<'_>],
    opts: &Options<'_>,
    imply_u_opts: ImplyUnstableOptions,
    cx: Context<'_>,
) -> Result<()> {
    let engine = e_opts.kind();

    let mut cmd = Command::new(engine.name());
    configure_early(&mut cmd, e_opts, krate, opts, cx)?;
    configure_late(&mut cmd, engine, extern_crates, opts);

    if let ImplyUnstableOptions::Yes = imply_u_opts
        && match probe_identity(opts) {
            Identity::True => cx.engine(engine).is_ok_and(|v| v.channel.allows_unstable()),
            Identity::Stable => false,
            Identity::Nightly => true,
        }
    {
        // FIXME: Should we offer an explicit opt out (e.g., via `-I*z` with * in "tsn")?
        cmd.arg("-Zunstable-options");
    }

    execute(cmd, opts.dbg_opts)?;

    Ok(())
}

pub(crate) fn query_engine_version(
    engine: EngineKind,
    toolchain: Option<&OsStr>,
    dbg_opts: DebugOptions,
) -> Result<Version<String>, EngineVersionError> {
    use EngineVersionError as Error;

    let mut cmd = Command::new(engine.name());

    // Must come first!
    configure_toolchain(&mut cmd, toolchain);

    cmd.arg("-V");

    // FIXME: Skip this execution if `-00`? (which doesn't exist yet).
    // FIXME: Setting dry_run explicitly is hacky.
    _ = gate(|p| render(&cmd, p), DebugOptions { dry_run: false, ..dbg_opts });

    let output = cmd.output().map_err(|_| Error::Other)?;

    if !output.status.success() {
        // We failed to obtain version information.
        // While that's very likely due to rustup complaining about some legitimate issues,
        // strictly speaking we don't know for sure -- it could be rust{,do}c acting up.
        //
        // We don't support rust{,do}c *wrappers* (via `RUST{,DO}C_WRAPPER`) at the time
        // of writing, so that's a source of problems we *don't* have!
        //
        // Ideally we would now try to query various programs that participated (mainly just
        // rustup that is) to find the culprit / root cause for better error reporting.
        // Sadly, rustup doesn't offer any mechanism to make it output machine-readable and
        // stable data. See also <https://github.com/rust-lang/rustup/issues/450>.
        //
        // So unfortunately we have no choice but to try and parse stderr output.
        //
        // FIXME: Avoid the need for `from_utf8`.
        if let Ok(stderr) = String::from_utf8(output.stderr)
            && let Some(line) = stderr.lines().next()
            && let Some(line) = line.strip_prefix("error: ")
        {
            // We now try to find the most common cases of rustup failure in order to
            // return a more concise error code. I don't want to forward the verbose and
            // roundabout error diagnostics.
            //
            // FIXME: Admittedly, these checks could be stricter by comparing the "variables"
            // (toolchain, component, …). In practice however it shouldn't really matter.

            if let Some(line) = line.strip_prefix("toolchain '") {
                if line.ends_with("' is not installable") {
                    return Err(Error::UnknownToolchain);
                }
                if line.ends_with("' is not installed") {
                    return Err(Error::ToolchainNotInstalled);
                }
            }

            if let Some((component, toolchain)) =
                line.split_once("' is not installed for the custom toolchain '")
                && component.starts_with('\'')
                && toolchain.ends_with("'.")
            {
                return Err(Error::UnavailableComponent);
            }
        }

        return Err(Error::Other);
    }

    let source = std::str::from_utf8(&output.stdout).map_err(|_| Error::Malformed)?;
    let source = source.strip_suffix('\n').ok_or(Error::Malformed)?;

    // The name of the binary *has* to exist for the version string to be considered valid!
    let (binary_name, source) = source.split_once(' ').ok_or(Error::Malformed)?;

    if binary_name != engine.name() {
        return Err(Error::Malformed);
    }

    match source {
        // Output by rust{,do}c if bootstrap didn't provide a version.
        "unknown" => Err(Error::Unknown),
        // This may happen if env var `RUSTC_OVERRIDE_VERSION_STRING` exists
        // and contains an invalid version as rust{,do}c outputs it verbatim.
        source => Version::parse(source).map(Version::into_owned).ok_or(Error::Malformed),
    }
}

#[derive(Clone, Copy)]
pub(crate) enum EngineVersionError {
    UnknownToolchain,
    ToolchainNotInstalled,
    UnavailableComponent,
    Unknown,
    Malformed,
    Other,
}

impl EngineVersionError {
    // FIXME: Figure out how much into detail we want go and how to best word these.
    //        E.g., do we want to dump underlying IO errors and parts of stderr output
    //        as emitted by `--version`?
    pub(crate) fn short_desc(self) -> &'static str {
        match self {
            Self::UnknownToolchain => "toolchain unknown",
            Self::ToolchainNotInstalled => "toolchain not installed",
            Self::UnavailableComponent => "component unavailable",
            Self::Unknown => "unknown",
            Self::Malformed => "malformed",
            Self::Other => "error",
        }
    }
}

pub(crate) fn query_crate_name(
    krate: Crate<'_>,
    opts: &Options<'_>,
    cx: Context<'_>,
) -> Result<CrateName<String>> {
    let engine = EngineOptions::Rustc(default());

    let mut cmd = Command::new(engine.kind().name());

    configure_early(&mut cmd, &engine, krate, opts, cx)?;

    cmd.arg("--print=crate-name");

    // FIXME: Only skip this execution if `-00` (which doesn't exist yet).
    match gate(|p| render(&cmd, p), opts.dbg_opts) {
        ControlFlow::Continue(()) => {}
        ControlFlow::Break(()) => {
            return Ok(CrateName::new_unchecked("UNKNOWN_DUE_TO_DRY_RUN".into()));
        }
    }

    // FIXME: Double check that `path`=`-` (STDIN) properly works with `output()` once we support that ourselves.
    let output = cmd.output()?;
    _ = output.stderr;

    // If we trigger this likely means we passed incorrect flags to the rustc invocation.
    // FIXME: Unfortunately, this can actually be triggered in practice under `d -@`
    // since we pass along verbatim flags as obtained by `compile-flags` directives
    // which may "erroneously" contain rustdoc-specific flags. See also
    // <https://github.com/rust-lang/rust/issues/137442>.
    // If the r-l/r doesn't go anywhere provide a mechanism(s) (via a flag) to
    // somehow remedy this situation (e.g., filtering flags or skipping given
    // directives).
    assert!(output.status.success(), "failed to properly query rustc about the crate name");

    let crate_name = String::from_utf8(output.stdout).map_err(drop).and_then(|mut output| {
        output.truncate(output.trim_end().len());
        CrateName::parse(output)
    });

    Ok(crate_name.expect("rustc provided an invalid crate name"))
}

/// Configure the engine invocation with options that it needs very early
/// (i.e., during certain print requests).
fn configure_early(
    cmd: &mut Command,
    e_opts: &EngineOptions<'_>,
    krate: Crate<'_>,
    opts: &Options<'_>,
    cx: Context<'_>,
) -> Result<()> {
    // Must come first!
    configure_toolchain(cmd, opts.toolchain);

    cmd.arg(krate.path);

    if let Some(name) = krate.name {
        cmd.arg("--crate-name");
        cmd.arg(name.as_str());
    }

    // FIXME: IINM older versions of rustdoc don't support this flag. Do something smarter
    //        in that case or at least emit a proper error diagnostic.
    if let Some(CrateType(typ)) = krate.typ {
        cmd.arg("--crate-type");
        cmd.arg(typ);
    }

    // Regarding crate name querying, the edition is vital. After all,
    // rustc needs to parse the crate root to find `#![crate_name]`.
    if let Some(edition) = krate.edition {
        let version = cx.engine(e_opts.kind()).map_err(|error| {
            emit_failed_to_obtain_version_for_opt(
                e_opts.kind(),
                error,
                fmt!("the requested edition `{}`", edition.to_str()),
            )
        })?;

        // FIXME: These dates and versions have been manually verified *with rustc*.
        //        It's possible that there are differences to rustdoc. Audit!
        let syntax = match version.channel {
            // FIXME: Unimplemented!
            Channel::Stable | Channel::Beta { prerelease: _ } => Some(Syntax::Edition),
            Channel::Nightly | Channel::Dev => match version.commit {
                Some(commit) => match () {
                    // <rust-lang/rust#50080> was merged on 2018-04-21T07:42Z.
                    // Thus it would've likely made it into *nightly-2018-04-22(2018-04-21).
                    // However, since some tools didn't build this nightly doesn't exist.
                    // In fact, nightly-2018-04-{20..26} don't exist, nightly-2018-04-27 is
                    // the first to feature `--edition`.
                    // Regardless, I like this more precise date better.
                    _ if commit.date >= D!(2018, 04, 21) => Some(Syntax::Edition),
                    // <rust-lang/rust#49035>
                    _ if commit.date >= D!(2018, 03, 23) => Some(Syntax::ZeeEdition),
                    // <rust-lang/rust#48014>
                    _ if commit.date >= D!(2018, 02, 07) => Some(Syntax::ZeeEpoch),
                    _ => None,
                },
                None => Some(Syntax::Edition), // FIXME: Unimplemented!
            },
        };

        let syntax = syntax.ok_or_else(|| {
            self::error(fmt!(
                "the version of the underyling `{}` does not support editions",
                e_opts.kind().name()
            ))
            .done()
        })?;

        match syntax {
            Syntax::Edition => {
                cmd.arg("--edition");
                cmd.arg(edition.to_str())
            }
            Syntax::ZeeEdition => cmd.arg(format!("-Zedition={}", edition.to_str())),
            Syntax::ZeeEpoch => cmd.arg(format!("-Zepoch={}", edition.to_str())),
        };

        enum Syntax {
            Edition,
            ZeeEdition,
            ZeeEpoch,
        }
    }

    // Regarding crate name querying, let's better honor this option
    // since it may significantly affect rustc's behavior.
    if let Some(identity) = opts.b_opts.identity {
        cmd.env("RUSTC_BOOTSTRAP", match identity {
            Identity::True => "0",
            // FIXME: Bail out with an error if we know that
            //        that engine version doesn't support this value.
            //        And warn in cases where it's not deducible
            Identity::Stable => "-1",
            // FIXME: In older versions, you had to set this env var
            //        to a "secret key" that comprised of a hash of
            //        several "bootstrap variables". Either support that
            //        smh. or throw an "unsupported" error.
            Identity::Nightly => "1",
        });
    }

    configure_v_opts(cmd, &opts.v_opts);
    configure_e_opts(cmd, e_opts, cx)?;

    if let Some(opts) = e_opts.kind().env_opts() {
        cmd.args(opts);
    }

    Ok(())
}

fn configure_toolchain(cmd: &mut Command, toolchain: Option<&OsStr>) {
    // FIXME: Consider only setting the (rustup) toolchain if the env var `RUSTUP_HOME` exists.
    //        And emitting a warning further up the stack of course.
    if let Some(toolchain) = toolchain {
        cmd.arg(toolchain);
    }
}

/// Configure the engine invocation with options that it doesn't need early
/// (i.e., during certain print requests).
fn configure_late(
    cmd: &mut Command,
    engine: EngineKind,
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
        let ExternCrate::Named { name, path, typ: _ } = extern_crate else {
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
        // FIXME: (low prio) Lower to `-Ztrait-solver=next` in older versions.
        cmd.arg("-Znext-solver");
    }

    if opts.b_opts.internals {
        // FIXME: Lower to `-Zverbose` in older versions.
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

fn configure_e_opts(cmd: &mut Command, e_opts: &EngineOptions<'_>, cx: Context<'_>) -> Result<()> {
    match e_opts {
        EngineOptions::Rustc(c_opts) => {
            if c_opts.check_only {
                // FIXME: Should we `-o $null`?
                cmd.arg("--emit=metadata");
            }

            if c_opts.shallow {
                let version = cx.engine(e_opts.kind()).map_err(|error| {
                    emit_failed_to_obtain_version_for_opt(
                        e_opts.kind(),
                        error,
                        fmt!("option `--shallow`"),
                    )
                })?;

                let syntax = match version.channel {
                    Channel::Stable => match () {
                        _ if version.triple >= V!(1, 85, 0) => Syntax::ZeeParseCrateRootOnly,
                        // FIXME: Find the *actual* lower bound.
                        _ => Syntax::ZeeParseOnly,
                    },
                    // FIXME: Unimplemented!
                    Channel::Beta { prerelease: _ } => Syntax::ZeeParseCrateRootOnly, // FIXME: Actually unimpl'ed!
                    Channel::Nightly | Channel::Dev => match version.commit {
                        Some(commit) => match () {
                            _ if commit.date >= D!(2024, 11, 29) => Syntax::ZeeParseCrateRootOnly,
                            // FIXME: Find the *actual* lower bound.
                            _ => Syntax::ZeeParseOnly,
                        },
                        None => match () {
                            _ if version.triple > V!(1, 85, 0) => Syntax::ZeeParseCrateRootOnly,
                            _ if version.triple == V!(1, 85, 0) => {
                                // FIXME: Improve wording. Actually print the version and print
                                //        the two candidates!
                                return Err(error(fmt!(
                                    "could not determine how to forward option `--shallow` to the underyling `{}`",
                                    e_opts.kind().name()
                                ))
                                .done()
                                .into());
                            }
                            // FIXME: Find the *actual* lower bound.
                            _ => Syntax::ZeeParseOnly,
                        },
                    },
                };

                enum Syntax {
                    ZeeParseCrateRootOnly,
                    ZeeParseOnly,
                }

                cmd.arg(match syntax {
                    Syntax::ZeeParseCrateRootOnly => "-Zparse-crate-root-only",
                    Syntax::ZeeParseOnly => "-Zparse-only",
                });
            }
        }
        EngineOptions::Rustdoc(d_opts) => {
            if let DocBackend::Json = d_opts.backend {
                cmd.arg("--output-format=json");
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
            }

            if d_opts.layout {
                cmd.arg("--show-type-layout");
            }

            if d_opts.link_to_def {
                cmd.arg("--generate-link-to-definition");
            }

            if d_opts.normalize {
                cmd.arg("-Znormalize-docs");
            }

            cmd.arg("--default-theme");
            cmd.arg(&d_opts.theme);

            cmd.args(&d_opts.v_opts.arguments);
        }
    }

    Ok(())
}

fn emit_failed_to_obtain_version_for_opt(
    engine: EngineKind,
    error: EngineVersionError,
    opt: impl Paint,
) -> EmittedError {
    fn diag(engine: EngineKind, error: EngineVersionError) -> Diagnostic {
        let diag = self::error(fmt!(
            "failed to retrieve the version of the underyling `{}`",
            engine.name()
        ));
        match error {
            EngineVersionError::Other => diag,
            // FIXME: Using the short description here is a bit awkward imo.
            _ => diag.note(fmt!("caused by: {}", error.short_desc())),
        }
    }

    diag(engine, error)
        .note(|p| {
            write!(p, "required in order to correctly forward ")?;
            opt(p)
        })
        .done()
}

pub(crate) fn run(
    program: impl AsRef<OsStr>,
    v_opts: &VerbatimOptions<'_>,
    dbg_opts: DebugOptions,
) -> io::Result<()> {
    let mut cmd = Command::new(program);
    configure_v_opts(&mut cmd, v_opts);
    execute(cmd, dbg_opts)
}

pub(crate) fn open(path: &Path, dbg_opts: DebugOptions) -> io::Result<()> {
    let message = |p: &mut diagnostic::Painter| {
        p.with(palette::COMMAND.on_default().bold(), |p| write!(p, "⟨open⟩ "))?;
        p.with(AnsiColor::Green, |p| write!(p, "{}", path.display()))
    };

    match gate(message, dbg_opts) {
        ControlFlow::Continue(()) => open::that(path),
        ControlFlow::Break(()) => Ok(()),
    }
}

#[must_use]
fn gate(message: impl Paint, dbg_opts: DebugOptions) -> ControlFlow<()> {
    if dbg_opts.verbose {
        let verb = if !dbg_opts.dry_run { "running" } else { "skipping" };
        debug(|p| {
            write!(p, "{verb} ")?;
            message(p)
        })
        .done();
    }

    match dbg_opts.dry_run {
        true => ControlFlow::Break(()),
        false => ControlFlow::Continue(()),
    }
}

fn execute(mut cmd: process::Command, dbg_opts: DebugOptions) -> io::Result<()> {
    match gate(|p| render(&cmd, p), dbg_opts) {
        ControlFlow::Continue(()) => cmd.status()?.exit_ok().map_err(io::Error::other),
        ControlFlow::Break(()) => Ok(()),
    }
}

pub(crate) fn probe_identity(opts: &Options<'_>) -> Identity {
    // FIXME: This doesn't take into account verbatim env vars (more specifically,
    //        `//@ rustc-env`).
    opts.b_opts.identity.or_else(environment::probe_identity).unwrap_or_default()
}

// This is very close to `<process::Command as fmt::Debug>::fmt` but prettier.
// FIXME: This lacks shell escaping!
fn render(cmd: &process::Command, p: &mut diagnostic::Painter) -> io::Result<()> {
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

/// Engine-specific build options.
pub(crate) enum EngineOptions<'a> {
    Rustc(CompileOptions),
    Rustdoc(DocOptions<'a>),
}

impl EngineOptions<'_> {
    pub(crate) fn kind(&self) -> EngineKind {
        match self {
            Self::Rustc(_) => EngineKind::Rustc,
            Self::Rustdoc(_) => EngineKind::Rustdoc,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum EngineKind {
    Rustc,
    Rustdoc,
}

impl EngineKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Rustc => "rustc",
            Self::Rustdoc => "rustdoc",
        }
    }

    const fn logging_env_var(self) -> &'static str {
        match self {
            Self::Rustc => "RUSTC_LOG",
            Self::Rustdoc => "RUSTDOC_LOG",
        }
    }

    fn env_opts(self) -> Option<&'static [String]> {
        match self {
            Self::Rustc => environment::rustc_options(),
            Self::Rustdoc => environment::rustdoc_options(),
        }
    }
}

mod palette {
    use anstyle::AnsiColor;

    pub(super) const VARIABLE: AnsiColor = AnsiColor::Yellow;
    pub(super) const COMMAND: AnsiColor = AnsiColor::Magenta;
    pub(super) const ARGUMENT: AnsiColor = AnsiColor::Green;
}

#[derive(Default)]
pub(crate) struct CompileOptions {
    pub(crate) check_only: bool,
    pub(crate) shallow: bool,
}

// FIXME: Remove once we have const Default
impl CompileOptions {
    pub(crate) const DEFAULT: Self = Self { check_only: false, shallow: false };
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

#[derive(Clone, Copy)]
pub(crate) struct DebugOptions {
    pub(crate) verbose: bool,
    pub(crate) dry_run: bool,
}

// FIXME: This type leads to such awkward code; consider remodeling it.
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum ExternCrate<'src> {
    Unnamed {
        path: Spanned<&'src str>,
        // FIXME: This field is only relevant for compiletest auxiliaries. Model this better.
        typ: Option<CrateType>,
    },
    Named {
        name: CrateName<&'src str>,
        path: Option<Spanned<Cow<'src, str>>>,
        // FIXME: This field is only relevant for compiletest auxiliaries. Model this better.
        typ: Option<CrateType>,
    },
}

#[derive(Clone)]
pub(crate) struct Options<'a> {
    pub(crate) toolchain: Option<&'a OsStr>,
    pub(crate) b_opts: &'a BuildOptions,
    pub(crate) v_opts: VerbatimOptions<'a>,
    pub(crate) dbg_opts: DebugOptions,
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

/// Whether to imply `-Zunstable-options` (for developer convenience).
#[derive(Clone, Copy)]
pub(crate) enum ImplyUnstableOptions {
    Yes,
    No,
}
