//! Low-level build routines.
//!
//! The high-level build operations are defined in [`crate::operate`].

use crate::{
    context::Context,
    data::{Channel, Crate, CrateName, CrateType, D, DocBackend, Identity, V, Version},
    diagnostic::{self, EmittedError, Paint, debug, error},
    error::Result,
    fmt,
    utility::default,
};
use anstyle::{AnsiColor, Effects};
use std::{
    borrow::Cow,
    ffi::OsStr,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{self, Command, ExitStatusError},
    string::FromUtf8Error,
};

mod environment;

pub(crate) fn perform(
    e_opts: &EngineOptions<'_>,
    krate: Crate<'_>,
    opts: &Options<'_>,
    imply_u_opts: ImplyUnstableOptions,
    cx: Context<'_>,
) -> Result<()> {
    let engine = e_opts.engine();

    let mut cmd = engine.command(cx).map_err(|error| error.emit())?;
    configure_early(&mut cmd, e_opts, krate, opts, cx)?;
    configure_late(&mut cmd, engine, opts, cx)?;

    if let ImplyUnstableOptions::Yes = imply_u_opts
        && match probe_identity(opts) {
            Identity::True => engine.version(cx).is_ok_and(|v| v.channel.allows_unstable()),
            Identity::Stable => false,
            Identity::Nightly => true,
        }
    {
        // FIXME: Should we offer an explicit opt out (e.g., via `-I*z` with * in "tsn")?
        cmd.arg("-Zunstable-options");
    }

    execute(cmd, cx).and_then(|res| res.map_err(io::Error::other))?;

    Ok(())
}

/// Don't call this directly! Use [`EngineKind::path`] instead.
fn query_engine_path(engine: Engine, cx: Context<'_>) -> Result<String, QueryEnginePathError> {
    use QueryEnginePathError as Error;

    // FIXME: Support non-rustup environments somehow (needs design work).
    let mut cmd = Command::new("rustup");
    if let Some(toolchain) = &cx.opts().toolchain {
        cmd.arg(toolchain);
    }
    cmd.arg("which");
    cmd.arg(engine.name());

    log_execution(|p| render(&cmd, p), cx);

    // FIXME: Forward underlying IO error (we can't rn, it doesn't impl `Clone`).
    let output = cmd.output().map_err(|_| Error::RustupSpawnFailure)?;

    if !output.status.success() {
        // Sadly, rustup doesn't offer any mechanism to make it output machine-readable and
        // stable data. See also <https://github.com/rust-lang/rustup/issues/450>.
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

        return Err(Error::RustupFailure);
    }

    // FIXME: Does `rustup` forward paths that aren't valid UTF-8 or does it error?
    let mut path = String::from_utf8(output.stdout).map_err(Error::InvalidPath)?;
    path.pop(); // drop trailing `\n`

    Ok(path)
}

#[derive(Clone)]
pub(crate) enum QueryEnginePathError {
    RustupSpawnFailure,
    UnknownToolchain,
    ToolchainNotInstalled,
    UnavailableComponent,
    RustupFailure,
    InvalidPath(FromUtf8Error),
}

impl QueryEnginePathError {
    // FIXME: Figure out how much into detail we want go and how to best word these.
    //        E.g., do we want to dump underlying IO errors and parts of stderr output?
    pub(crate) fn short_desc(self) -> &'static str {
        match self {
            Self::RustupSpawnFailure => "rustup unavailable",
            Self::RustupFailure | Self::InvalidPath(_) => "error",
            Self::UnknownToolchain => "toolchain unknown",
            Self::ToolchainNotInstalled => "toolchain not installed",
            Self::UnavailableComponent => "component unavailable",
        }
    }

    fn emit(self) -> EmittedError {
        // FIMXE: Print actual name of the engine.
        let error = error(fmt!("failed to obtain path to rust{{,do}}c"));

        match self {
            // FIMXE: Print underlying IO error cause (we can't thread it thru rn cuz io::Error doesn't impl `Clone`
            //        but `Self` unfortunately requires `Clone` due to the "query system" impl needing it atm).
            Self::RustupSpawnFailure => error.note(fmt!("failed to execute `rustup`")),
            Self::UnknownToolchain | Self::ToolchainNotInstalled | Self::UnavailableComponent => {
                error.note(fmt!("{}", self.short_desc()))
            }
            Self::RustupFailure => error.note(fmt!("`rustup` exited unsuccessfully")),
            Self::InvalidPath(cause) => {
                error.note(fmt!("`rustup` provided a non-UTF-8 path: {cause}"))
            }
        }
        .done()
    }
}

/// Don't call this directly! Use [`EngineKind::version`] instead.
fn query_engine_version(
    engine: Engine,
    cx: Context<'_>,
) -> Result<Version<String>, QueryEngineVersionError> {
    use QueryEngineVersionError as Error;

    let mut cmd = engine.command(cx).map_err(Error::EnginePathError)?;

    cmd.arg("-V");

    log_execution(|p| render(&cmd, p), cx);

    let output = cmd.output().map_err(|_| Error::EngineSpawnFailure)?;

    if !output.status.success() {
        return Err(Error::EngineFailure);
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

#[derive(Clone)]
pub(crate) enum QueryEngineVersionError {
    EngineFailure,
    EnginePathError(QueryEnginePathError),
    EngineSpawnFailure,
    Malformed,
    Unknown,
}

impl QueryEngineVersionError {
    // FIXME: Figure out how much into detail we want go and how to best word these.
    //        E.g., do we want to dump underlying IO errors and parts of stderr output
    //        as emitted by `--version`?
    pub(crate) fn short_desc(self) -> &'static str {
        match self {
            Self::EnginePathError(error) => error.short_desc(),
            Self::Unknown => "unknown",
            Self::Malformed => "malformed",
            // FIXME: Print actual engine name
            Self::EngineFailure => "`rust{,do}c` exited unsuccessfully",
            Self::EngineSpawnFailure => "failed to execute `rustdo{,do}c`",
        }
    }
}

pub(crate) fn query_crate_name<'a>(
    krate: Crate<'a>,
    opts: &Options<'_>,
    cx: Context<'_>,
) -> Result<CrateName<Cow<'a, str>>, QueryCrateNameError> {
    use QueryCrateNameError as Error;

    // If we've been explicitly given a crate name, just use it. It doesn't matter if it
    // doesn't match the `#![crate_name]` since we assume that callers of this function
    // have already tried executing rust{,do}c under the same configuration `(krate, opts)`
    // meaning we should've already bailed out on a mismatch reported by the engine.
    //
    // This isn't just a fast path: Querying rustc can fail in practice if the user has
    // provided us with an invalid configuration (1); this allows them to quickly suppress
    // this query by manually providing an "overwrite".
    if let Some(name) = krate.name {
        return Ok(name.into());
    }

    let e_opts = EngineOptions::Rustc(default());

    let mut cmd = e_opts.engine().command(cx).map_err(Error::EnginePathError)?;

    // FIXME: Make this function return a more structured error and then get rid of `Error::Other`.
    configure_early(&mut cmd, &e_opts, krate, opts, cx).map_err(Error::Other)?;

    cmd.arg("--print=crate-name");

    log_execution(|p| render(&cmd, p), cx);

    // FIXME: Double check that `path`=`-` (STDIN) properly works with `output()` once we support that ourselves.
    let mut output = cmd.output().map_err(Error::RustcSpawnFailure)?;
    _ = output.stderr;

    if !output.status.success() {
        // (1) This likely means we passed incorrect flags to the rustc invocation which
        // can happen in practice if the user knowingly or unknowingly provided verbatim
        // flags that don't make for `rustc --print=crate-name`.
        //
        // A common example at the time of writing would be rustdoc tests using
        // `//@ compile-flags` for rustdoc-specific flags instead of `//@ doc-flags`.
        // See also <https://github.com/rust-lang/rust/issues/137442>.
        //
        // We (obviously) have to pass along any rustc-specific verbatim flags because
        // they could contain `--crate-name`, `--edition` (albeit master compiletest
        // actually rejects this since recently and we now need to do so, too, sadly
        // but we haven't yet) which influence the crate name.

        // // FIXME: Maybe forward rustc's stderr under `-vv` (`-vv` not allowed yet).
        return Err(Error::RustcFailure);
    }

    // Trim the trailing line break.
    output.stdout.truncate(output.stdout.trim_ascii_end().len());

    let name = String::from_utf8(output.stdout).map_err(Error::InvalidUtf8)?;

    CrateName::parse(name).map(Into::into).map_err(Error::InvalidCrateName)
}

pub(crate) enum QueryCrateNameError {
    EnginePathError(QueryEnginePathError),
    RustcSpawnFailure(io::Error),
    RustcFailure,
    InvalidUtf8(FromUtf8Error),
    InvalidCrateName(String),
    Other(crate::error::Error),
}

impl QueryCrateNameError {
    pub(crate) fn emit(self) -> EmittedError {
        let error = error(fmt!("failed to obtain the crate name from rustc"));

        match self {
            // FIXME: embed inner error inside the one above (indented)
            Self::EnginePathError(error) => return error.emit(),
            Self::RustcSpawnFailure(cause) => {
                error.note(fmt!("failed to execute `rustc`: {cause}"))
            }
            Self::RustcFailure => {
                // FIXME: Attempt to better explain what's going on, see (1).
                error
                    .note(fmt!(
                        "`rustc` exited unsuccessfully (likely due to invalid flags passed to it)"
                    ))
                    .help(fmt!("try rerunning with `-n<NAME>` set to bypass this logic"))
            }
            Self::InvalidUtf8(cause) => error.note(|p| {
                write!(p, "`rustc` provided `")?;
                p.write_all(cause.as_bytes())?;
                write!(p, "` which is not valid UTF-8")
            }),
            Self::InvalidCrateName(name) => {
                error.note(fmt!("`rustc` provided `{name}` which is not a valid crate name"))
            }
            Self::Other(error) => return error.emit(),
        }
        .done()
    }
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
    if let Some(path) = krate.path {
        cmd.arg(path);
    }

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
        let version = e_opts.engine().version(cx).map_err(|error| {
            emit_failed_to_obtain_version_for_opt(
                e_opts.engine(),
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
                    () if commit.date >= D!(2018, 04, 21) => Some(Syntax::Edition),
                    // <rust-lang/rust#49035>
                    () if commit.date >= D!(2018, 03, 23) => Some(Syntax::ZeeEdition),
                    // <rust-lang/rust#48014>
                    () if commit.date >= D!(2018, 02, 07) => Some(Syntax::ZeeEpoch),
                    () => None,
                },
                None => Some(Syntax::Edition), // FIXME: Unimplemented!
            },
        };

        let syntax = syntax.ok_or_else(|| {
            self::error(fmt!(
                "the version of the underyling `{}` does not support editions",
                e_opts.engine().name()
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
        cmd.env(
            "RUSTC_BOOTSTRAP",
            match identity {
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
            },
        );
    }

    configure_v_opts(cmd, &opts.v_opts);
    configure_e_opts(cmd, e_opts, cx)?;

    if let Some(opts) = e_opts.engine().env_opts() {
        cmd.args(opts);
    }

    Ok(())
}

/// Configure the engine invocation with options that it doesn't need early
/// (i.e., during certain print requests).
fn configure_late(
    cmd: &mut Command,
    engine: Engine,
    opts: &Options<'_>,
    cx: Context<'_>,
) -> Result<()> {
    // The crate name can't depend on any dependency crates, it's fine to skip this.
    // The opposite used to be the case actually prior to rust-lang/rust#117584.
    // E.g., via `#![crate_name = dependency::generate!()]`.
    // This no longer works and will be a hard error soon (rust-lang/rust#127581).
    // Lastly, it's impossible for a macro to generate inner attributes (rust-lang/rust#66920)
    // and even if that were to change at some point, rustc will never expand macros
    // in order to find `#![crate_name]` (ruled by T-lang).

    // FIXME: Only add this when requested by `operate`.
    cmd.arg("-Lcrate=.");

    if !opts.b_opts.extern_crates.is_empty() {
        for ext in &opts.b_opts.extern_crates {
            cmd.arg("--extern");
            cmd.arg(ext);
        }
    }

    // The crate name can't depend on any cfgs, it's fine to skip this.
    // In the past this wasn't really the case but since 1.83
    // `#[cfg_attr(…, crate_name = "…")]` is a hard error (if the spec holds).
    // See also rust-lang/rust#91632.
    for cfg in &opts.b_opts.cfgs {
        cmd.arg("--cfg");
        cmd.arg(cfg);
    }
    // FIXME: This shouldn't be done here.
    if let Some(revision) = &opts.b_opts.revision {
        cmd.arg("--cfg");
        cmd.arg(revision);
    }

    for feature in &opts.b_opts.unstable_features {
        // NOTE: If <https://github.com/rust-lang/rfcs/pull/3791> gets accepted and implemented,
        //       we need to start switching on the version and change the lowering.
        cmd.arg(format!("-Zcrate-attr=feature({feature})"));
    }

    if opts.b_opts.suppress_lints {
        cmd.arg("--cap-lints=allow");
    }

    if opts.b_opts.next_solver {
        // NOTE: I won't bother with handling older syntaxes (like `-Ztrait-solver=next`)
        //       because `rrx`'s `-N` won't stay around for long anyway (it's only temporary).
        cmd.arg("-Znext-solver");
    }

    if opts.b_opts.internals {
        let version = engine.version(cx).map_err(|error| {
            emit_failed_to_obtain_version_for_opt(engine, error, fmt!("option `--internals`"))
        })?;

        let syntax = match version.channel {
            Channel::Stable => match () {
                () if version.triple >= V!(1, 77, 0) => Syntax::ZeeVerboseInternals,
                // FIXME Find the *actual* lower bound.
                () => Syntax::ZeeVerbose,
            },
            Channel::Beta { prerelease: _ } => Syntax::ZeeVerboseInternals, // FIXME: actually unimpl'ed!
            Channel::Nightly | Channel::Dev => match version.commit {
                Some(commit) => match () {
                    () if commit.date >= D!(2023, 12, 26) => Syntax::ZeeVerboseInternals,
                    // FIXME: Find the *actual* lower bound.
                    () => Syntax::ZeeVerbose,
                },
                None => match () {
                    () if version.triple > V!(1, 77, 0) => Syntax::ZeeVerboseInternals,
                    () if version.triple == V!(1, 77, 0) => {
                        // FIXME: Improve wording (print the two candidates and the version).
                        return Err(error(fmt!(
                            "could not determine how to forward option `--internals` to the underlying `{}`",
                            engine.name()
                        ))
                        .done()
                        .into());
                    }
                    // FIXME: Find the *actual* lower bound
                    () => Syntax::ZeeVerbose,
                },
            },
        };

        enum Syntax {
            ZeeVerbose,
            ZeeVerboseInternals,
        }

        cmd.arg(match syntax {
            Syntax::ZeeVerbose => "-Zverbose",
            Syntax::ZeeVerboseInternals => "-Zverbose-internals",
        });
    }

    if opts.b_opts.no_dedupe {
        cmd.arg("-Zdeduplicate-diagnostics=no");
    }

    if opts.b_opts.no_backtrace {
        cmd.env("RUST_BACKTRACE", "0");
    }

    // The logging output would just get thrown away.
    if let Some(filter) = &opts.b_opts.log {
        cmd.env(engine.logging_env_var(), filter);
    }

    Ok(())
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
                let version = e_opts.engine().version(cx).map_err(|error| {
                    emit_failed_to_obtain_version_for_opt(
                        e_opts.engine(),
                        error,
                        fmt!("option `--shallow`"),
                    )
                })?;

                let syntax = match version.channel {
                    Channel::Stable => match () {
                        () if version.triple >= V!(1, 85, 0) => Syntax::ZeeParseCrateRootOnly,
                        // FIXME: Find the *actual* lower bound.
                        () => Syntax::ZeeParseOnly,
                    },
                    // FIXME: Unimplemented!
                    Channel::Beta { prerelease: _ } => Syntax::ZeeParseCrateRootOnly, // FIXME: Actually unimpl'ed!
                    Channel::Nightly | Channel::Dev => match version.commit {
                        Some(commit) => match () {
                            () if commit.date >= D!(2024, 11, 29) => Syntax::ZeeParseCrateRootOnly,
                            // FIXME: Find the *actual* lower bound.
                            () => Syntax::ZeeParseOnly,
                        },
                        None => match () {
                            () if version.triple > V!(1, 85, 0) => Syntax::ZeeParseCrateRootOnly,
                            () if version.triple == V!(1, 85, 0) => {
                                // FIXME: Improve wording. Actually print the version and print
                                //        the two candidates!
                                return Err(error(fmt!(
                                    "could not determine how to forward option `--shallow` to the underyling `{}`",
                                    e_opts.engine().name()
                                ))
                                .done()
                                .into());
                            }
                            // FIXME: Find the *actual* lower bound.
                            () => Syntax::ZeeParseOnly,
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
    engine: Engine,
    error: QueryEngineVersionError,
    opt: impl Paint,
) -> EmittedError {
    self::error(fmt!("failed to retrieve the version of the underyling `{}`", engine.name()))
        .note(fmt!("caused by: {}", error.short_desc()))
        .note(|p| {
            write!(p, "required in order to correctly forward ")?;
            opt(p)
        })
        .done()
}

pub(crate) fn run(
    program: impl AsRef<OsStr>,
    v_opts: &VerbatimOptions<'_>,
    cx: Context<'_>,
) -> io::Result<Result<(), ExitStatusError>> {
    let mut cmd = Command::new(program);
    configure_v_opts(&mut cmd, v_opts);
    execute(cmd, cx)
}

pub(crate) fn open(path: &Path, cx: Context<'_>) -> io::Result<()> {
    let message = |p: &mut diagnostic::Painter| {
        p.with(palette::COMMAND.on_default().bold(), |p| write!(p, "⟨open⟩ "))?;
        p.with(AnsiColor::Green, |p| write!(p, "{}", path.display()))
    };

    log_execution(message, cx);
    open::that(path)
}

fn log_execution(message: impl Paint, cx: Context<'_>) {
    if !cx.opts().dbg_opts.verbose {
        return;
    }
    #[rustfmt::skip]
    debug(|p| { write!(p, "running ")?; message(p) }).done();
}

fn execute(mut cmd: process::Command, cx: Context<'_>) -> io::Result<Result<(), ExitStatusError>> {
    log_execution(|p| render(&cmd, p), cx);
    Ok(cmd.status()?.exit_ok())
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
    pub(crate) fn engine(&self) -> Engine {
        match self {
            Self::Rustc(_) => Engine::Rustc,
            Self::Rustdoc(_) => Engine::Rustdoc,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Engine {
    Rustc,
    Rustdoc,
}

impl Engine {
    const fn name(self) -> &'static str {
        match self {
            Self::Rustc => "rustc",
            Self::Rustdoc => "rustdoc",
        }
    }

    // FIXME: Investigate if we should also set RUSTC_LOG for rustdoc or if it doesn't make a difference.
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

    fn path(self, cx: Context<'_>) -> Result<String, QueryEnginePathError> {
        crate::context::invoke!(cx.query_engine_path(self))
    }

    fn command(self, cx: Context<'_>) -> Result<Command, QueryEnginePathError> {
        let path = PathBuf::from(self.path(cx)?);
        let mut cmd = Command::new(&path);
        // Very old versions (e.g, 1.0 and 1.2) can't find some of their shared libraries if we don't do this.
        // FIXME: We assume that the path is of the form "$PREFIX/bin/$FILE" and that the corresponding library
        //        folder is "$PREFIX/lib/". This relies on the likely undocumented/unstable file structure of
        //        rustup toolchains.
        if let Some(path) = path.ancestors().nth(const { ["bin", "*"].len() }) {
            // FIXME: This likely doesn't work on non-*nix OSes.
            // NOTE: We should probably *append* onto the library path instead of overwriting it completely.
            //       However, I've yet to find an issue with the current approach in our specific scenario.
            cmd.env("LD_LIBRARY_PATH", path.join("lib"));
        }
        Ok(cmd)
    }

    // Reminder: You can set the env var `RUSTC_OVERRIDE_VERSION_STRING` to
    // overwrite the version output by rust{,do}c (for the purpose of testing).
    pub(crate) fn version(
        self,
        cx: Context<'_>,
    ) -> Result<Version<String>, QueryEngineVersionError> {
        crate::context::invoke!(cx.query_engine_version(self))
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

#[derive(Clone)] // FIXME: This is awful!
#[allow(clippy::struct_excessive_bools)] // not worth to address
pub(crate) struct BuildOptions {
    pub(crate) cfgs: Vec<String>,
    pub(crate) revision: Option<String>,
    pub(crate) unstable_features: Vec<String>,
    pub(crate) extern_crates: Vec<String>,
    pub(crate) suppress_lints: bool,
    pub(crate) internals: bool,
    pub(crate) next_solver: bool,
    pub(crate) identity: Option<Identity>,
    pub(crate) no_dedupe: bool,
    pub(crate) log: Option<String>,
    pub(crate) no_backtrace: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct DebugOptions {
    pub(crate) verbose: bool,
}

#[derive(Clone)] // FIXME: This if awful!
pub(crate) struct Options<'a> {
    pub(crate) b_opts: BuildOptions,
    pub(crate) v_opts: VerbatimOptions<'a>,
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
