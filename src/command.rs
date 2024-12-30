//! Low-level build commands.
//!
//! The high-level build operations are defined in [`crate::operate`].

// Note that we try to avoid generating unnecessary flags where possible even if that means
// doing more work on our side. The main motivation for this is being able to just copy/paste
// the commands printed by `--verbose` for use in GitHub discussions without requiring any
// manual minimization.
// FIXME: Also mention to reduce conflicts with compile flags passed via `compiletest`
//        as well as those passed via the `RUST{,DOC}FLAGS` env vars.

use crate::{
    data::{CrateName, CrateNameRef, CrateType, DocBackend, Edition, Identity},
    diagnostic::{self, emit},
    error::Result,
    interface,
    utility::default,
};
use anstyle::{AnsiColor, Effects};
use std::{
    borrow::Cow,
    ffi::OsStr,
    io::{self, Write},
    ops::{Deref, DerefMut},
    path::Path,
    process,
};

mod environment;

pub(super) fn compile(
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    extern_crates: &[ExternCrate<'_>],
    flags: Flags<'_>,
    strictness: Strictness,
) -> Result {
    let mut cmd = Command::new("rustc", flags.debug, strictness);
    cmd.set_toolchain(flags);
    cmd.arg(path);

    cmd.set_crate_type(crate_type);
    cmd.set_crate_name(crate_name, path);
    cmd.set_edition(edition);

    cmd.set_extern_crates(extern_crates);

    cmd.set_cfgs(flags.build);
    cmd.set_rustc_features(flags.build);
    cmd.set_cap_lints(flags.build);
    cmd.set_next_solver(flags.build);

    cmd.set_internals_mode(flags.build);

    cmd.set_verbatim_flags(flags.verbatim);

    if let Some(flags) = environment::rustc_flags() {
        cmd.args(flags);
    }

    cmd.set_identity(flags.build);
    cmd.set_backtrace_behavior(flags.build);

    if let Some(filter) = &flags.build.log {
        cmd.env("RUSTC_LOG", filter);
    }

    cmd.execute()
}

pub(super) fn document(
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    extern_crates: &[ExternCrate<'_>],
    flags: Flags<'_>,
    // FIXME: temporary; integrate into flags: Flags<D> above (D discriminant)
    doc_flags: &interface::DocFlags,
    strictness: Strictness,
) -> Result {
    let mut cmd = Command::new("rustdoc", flags.debug, strictness);
    cmd.set_toolchain(flags);
    cmd.arg(path);

    cmd.set_crate_name(crate_name, path);
    cmd.set_crate_type(crate_type);
    cmd.set_edition(edition);

    cmd.set_extern_crates(extern_crates);

    if let DocBackend::Json = doc_flags.backend {
        cmd.arg("--output-format=json");
        cmd.uses_unstable_options = true;
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
        cmd.uses_unstable_options = true;
    }

    if doc_flags.layout {
        cmd.arg("--show-type-layout");
        cmd.uses_unstable_options = true;
    }

    if doc_flags.link_to_definition {
        cmd.arg("--generate-link-to-definition");
        cmd.uses_unstable_options = true;
    }

    if doc_flags.normalize {
        cmd.arg("-Znormalize-docs");
    }

    cmd.arg("--default-theme");
    cmd.arg(&doc_flags.theme);

    cmd.set_cfgs(flags.build);
    cmd.set_rustc_features(flags.build);
    cmd.set_cap_lints(flags.build);
    cmd.set_next_solver(flags.build);

    cmd.set_internals_mode(flags.build);

    cmd.set_verbatim_flags(flags.verbatim);

    if let Some(flags) = environment::rustdoc_flags() {
        cmd.args(flags);
    }

    cmd.set_identity(flags.build);
    cmd.set_backtrace_behavior(flags.build);

    if let Some(filter) = &flags.build.log {
        cmd.env("RUSTDOC_LOG", filter);
    }

    cmd.execute()
}

pub(crate) fn execute(program: impl AsRef<OsStr>, flags: &interface::DebugFlags) -> Result {
    Command::new(program, flags, Strictness::Strict).execute()
}

pub(crate) fn open(crate_name: CrateNameRef<'_>, flags: &interface::DebugFlags) -> Result {
    let path = std::env::current_dir()?.join("doc").join(crate_name.as_str()).join("index.html");

    if flags.verbose {
        emit!(Note(|p| {
            let verb = if !flags.dry_run { "running" } else { "skipping" };
            write!(p, "{verb} ")?;
            p.with(palette::COMMAND.on_default().effects(Effects::BOLD), |p| {
                write!(p, "⟨browser⟩ ")
            })?;
            p.with(AnsiColor::Green, |p| write!(p, "{}", path.display()))
        }));
    }

    if !flags.dry_run {
        open::that(path)?;
    }

    Ok(())
}

struct Command<'a> {
    inner: process::Command,
    flags: &'a interface::DebugFlags,
    strictness: Strictness,
    uses_unstable_options: bool,
}

impl<'a> Command<'a> {
    fn new(
        program: impl AsRef<OsStr>,
        flags: &'a interface::DebugFlags,
        strictness: Strictness,
    ) -> Self {
        Self {
            inner: process::Command::new(program),
            flags,
            strictness,
            uses_unstable_options: false,
        }
    }

    fn execute(mut self) -> Result {
        self.set_unstable_options();

        self.print(); // FIXME partially inline this
        if !self.flags.dry_run {
            self.status()?.exit_ok()?;
        }

        Ok(())
    }

    fn print(&self) {
        if !self.flags.verbose {
            return;
        }

        emit!(Note(|p| {
            let verb = if !self.flags.dry_run { "running" } else { "skipping" };
            write!(p, "{verb} ")?;
            self.render_into(p)
        }));
    }

    fn set_toolchain(&mut self, flags: Flags<'_>) {
        // FIXME: Consider only setting the (rustup) toolchain if the env var `RUSTUP_HOME` exists.
        //        And emitting a warning further up the stack of course.
        if let Some(toolchain) = flags.toolchain {
            self.arg(toolchain);
        }
    }

    fn set_crate_name(&mut self, crate_name: CrateNameRef<'_>, path: &Path) {
        if let Ok(fiducial_crate_name) = CrateName::adjust_and_parse_file_path(path)
            && crate_name == fiducial_crate_name.as_ref()
        {
            return;
        }

        self.arg("--crate-name");
        self.arg(crate_name.as_str());
    }

    fn set_crate_type(&mut self, crate_type: CrateType) {
        if crate_type == default() {
            return;
        }

        self.arg("--crate-type");
        self.arg(crate_type.to_str());
    }

    fn set_edition(&mut self, edition: Edition) {
        if edition == Edition::RUSTC_DEFAULT {
            return;
        }
        if !edition.is_stable() {
            self.uses_unstable_options = true;
        }

        self.arg("--edition");
        self.arg(edition.to_str());
    }

    fn set_extern_crates(&mut self, extern_crates: &[ExternCrate<'_>]) {
        // FIXME: should we skip this if Strictness::Strict?
        // What does `compiletest` do?
        if !extern_crates.is_empty() {
            // FIXME: Does this work with proc macro deps? I think so?
            self.arg("-Lcrate=.");
        }

        for extern_crate in extern_crates {
            let ExternCrate::Named { name, path } = extern_crate else {
                continue;
            };

            self.arg("--extern");
            match path {
                Some(path) => self.arg(format!("{name}={path}")),
                None => self.arg(name.as_str()),
            };
        }
    }

    fn set_internals_mode(&mut self, flags: &interface::BuildFlags) {
        if flags.rustc_verbose_internals {
            self.arg("-Zverbose-internals");
        }
    }

    fn set_identity(&mut self, flags: &interface::BuildFlags) {
        let Some(identity) = flags.identity else { return };
        self.env("RUSTC_BOOTSTRAP", match identity {
            Identity::True => "0",
            Identity::Stable => "-1",
            Identity::Nightly => "1",
        });
    }

    fn set_backtrace_behavior(&mut self, flags: &interface::BuildFlags) {
        if flags.no_backtrace {
            self.env("RUST_BACKTRACE", "0");
        }
    }

    fn set_cfgs(&mut self, flags: &interface::BuildFlags) {
        for cfg in &flags.cfgs {
            self.arg("--cfg");
            self.arg(cfg);
        }
        for feature in &flags.cargo_features {
            // FIXME: Warn on conflicts with `cfgs` from `self.arguments.cfgs`.
            // FIXME: collapse
            self.arg("--cfg");
            self.arg(format!("feature=\"{feature}\""));
        }
    }

    fn set_rustc_features(&mut self, flags: &interface::BuildFlags) {
        for feature in &flags.rustc_features {
            self.arg(format!("-Zcrate-attr=feature({feature})"));
        }
    }

    fn set_cap_lints(&mut self, flags: &interface::BuildFlags) {
        if flags.cap_lints {
            self.arg("--cap-lints=warn");
        }
    }

    fn set_next_solver(&mut self, flags: &interface::BuildFlags) {
        if flags.next_solver {
            self.arg("-Znext-solver");
        }
    }

    fn set_unstable_options(&mut self) {
        if let Strictness::Lenient = self.strictness
            && self.uses_unstable_options
        {
            self.arg("-Zunstable-options");
        }
    }

    fn set_verbatim_flags(&mut self, flags: VerbatimFlags<'_>) {
        for (key, value) in flags.environment {
            match value {
                Some(value) => self.env(key, value),
                None => self.env_remove(key),
            };
        }
        self.args(flags.arguments);
    }
}

impl Deref for Command<'_> {
    type Target = process::Command;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Command<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

trait CommandExt {
    fn render_into(&self, p: &mut diagnostic::Painter) -> io::Result<()>;
}

// This is very close to `<process::Command as fmt::Debug>::fmt` but prettier.
// FIXME: This lacks shell escaping!
impl CommandExt for process::Command {
    fn render_into(&self, p: &mut diagnostic::Painter) -> io::Result<()> {
        #[allow(irrefutable_let_patterns)]
        if let envs = self.get_envs()
            && !envs.is_empty()
        {
            p.set(palette::VARIABLE)?;
            for (key, value) in self.get_envs() {
                // FIXME: Print `env -u VAR` for removed vars before
                // added vars just like `Command`'s `Debug` impl.
                let Some(value) = value else { continue };

                p.with(Effects::BOLD, |p| write!(p, "{}", key.display()))?;
                write!(p, "={} ", value.display())?;
            }
            p.unset()?;
        }

        p.with(palette::COMMAND.on_default().effects(Effects::BOLD), |p| {
            write!(p, "{}", self.get_program().display())
        })?;

        for argument in self.get_args() {
            p.with(palette::ARGUMENT, |p| write!(p, " {}", argument.display()))?;
        }

        Ok(())
    }
}

mod palette {
    use anstyle::AnsiColor;

    pub(super) const VARIABLE: AnsiColor = AnsiColor::Yellow;
    pub(super) const COMMAND: AnsiColor = AnsiColor::Magenta;
    pub(super) const ARGUMENT: AnsiColor = AnsiColor::Green;
}

#[derive(Clone)]
pub(crate) enum ExternCrate<'src> {
    Unnamed { path: &'src str },
    Named { name: CrateNameRef<'src>, path: Option<Cow<'src, str>> },
}

// FIXME: Should we / can we move this into `cli` somehow?
#[derive(Clone, Copy)]
pub(crate) struct Flags<'a> {
    pub(crate) toolchain: Option<&'a OsStr>,
    pub(crate) build: &'a interface::BuildFlags,
    pub(crate) verbatim: VerbatimFlags<'a>,
    pub(crate) debug: &'a interface::DebugFlags,
}

// FIXME: bad name, env is not "flags"
#[derive(Clone, Copy)]
pub(crate) struct VerbatimFlags<'a> {
    pub(crate) arguments: &'a [&'a str],
    pub(crate) environment: &'a [(&'a str, Option<&'a str>)],
}

// FIXME: bad name, env is not "flags"
#[derive(Clone, Default)]
pub(crate) struct VerbatimFlagsBuf<'a> {
    pub(crate) arguments: Vec<&'a str>,
    pub(crate) environment: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> VerbatimFlagsBuf<'a> {
    pub(crate) fn extended(mut self, other: VerbatimFlags<'a>) -> Self {
        self.arguments.extend_from_slice(other.arguments);
        self.environment.extend_from_slice(other.environment);
        self
    }

    pub(crate) fn as_ref(&self) -> VerbatimFlags<'_> {
        VerbatimFlags { arguments: &self.arguments, environment: &self.environment }
    }
}

pub(crate) enum Strictness {
    Strict,
    Lenient,
}
