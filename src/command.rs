//! Low-level build commands.
//!
//! The high-level build commands are defined in [`crate::builder`].

// Note that we try to avoid generating unnecessary flags where possible even if that means
// doing more work on our side. The main motivation for this is being able to just copy/paste
// the commands printed by `--verbose` for use in GitHub discussions without requiring any
// manual minimization.
// FIXME: Also mention to reduce conflicts with compile flags passed via `compiletest`
//        as well as those passed via the `RUST{,DOC}FLAGS` env vars.

use crate::{
    cli::{self, InputPath},
    data::{CrateNameCow, CrateNameRef, CrateType, Edition},
    diagnostic::info,
    error::Result,
    utility::default,
};
use owo_colors::OwoColorize;
use std::{
    borrow::Cow,
    ffi::OsStr,
    fmt,
    ops::{Deref, DerefMut},
    process,
};

mod environment;

pub(crate) fn compile(
    path: InputPath<'_>,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    extern_crates: &[ExternCrate<'_>],
    flags: Flags<'_>,
    strictness: Strictness,
) -> Result {
    let mut command = Command::new("rustc", flags.program, strictness);
    command.set_toolchain(flags);
    command.arg(path.into_inner()); // rustc supports `-`

    command.set_crate_type(crate_type);
    command.set_crate_name(crate_name, path);
    command.set_edition(edition);

    command.set_extern_crates(extern_crates);

    command.set_cfgs(flags.build);
    command.set_rustc_features(flags.build);
    command.set_cap_lints(flags.build);
    command.set_internals_mode(flags.build);

    command.set_verbatim_flags(flags.verbatim);

    if let Some(flags) = environment::rustc_flags() {
        command.args(flags);
    }

    command.set_backtrace_behavior(flags.build);

    if let Some(filter) = &flags.build.log {
        // FIXME: DRY default value. Do this inside `cli` once it uses the builder pattern.
        command.env("RUSTC_LOG", filter.as_deref().unwrap_or("debug"));
    }

    command.execute()
}

pub(crate) fn document(
    path: InputPath<'_>,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    extern_crates: &[ExternCrate<'_>],
    flags: Flags<'_>,
    strictness: Strictness,
) -> Result {
    let mut command = Command::new("rustdoc", flags.program, strictness);
    command.set_toolchain(flags);
    command.arg(path.into_inner()); // rustdoc supports `-`

    command.set_crate_name(crate_name, path);
    command.set_crate_type(crate_type);
    command.set_edition(edition);

    command.set_extern_crates(extern_crates);

    if flags.build.json {
        command.arg("--output-format");
        command.arg("json");
        command.uses_unstable_options = true;
    }

    if flags.build.private {
        command.arg("--document-private-items");
    }

    if flags.build.hidden {
        command.arg("--document-hidden-items");
        command.uses_unstable_options = true;
    }

    if flags.build.layout {
        command.arg("--show-type-layout");
        command.uses_unstable_options = true;
    }

    if flags.build.link_to_definition {
        command.arg("--generate-link-to-definition");
        command.uses_unstable_options = true;
    }

    if flags.build.normalize {
        command.arg("-Znormalize-docs");
    }

    if let Some(crate_version) = &flags.build.crate_version {
        command.arg("--crate-version");
        command.arg(crate_version);
    }

    command.arg("--default-theme");
    command.arg(&flags.build.theme);

    command.set_cfgs(flags.build);
    command.set_rustc_features(flags.build);
    command.set_cap_lints(flags.build);
    command.set_internals_mode(flags.build);

    command.set_verbatim_flags(flags.verbatim);

    if let Some(flags) = environment::rustdoc_flags() {
        command.args(flags);
    }

    command.set_backtrace_behavior(flags.build);

    if let Some(filter) = &flags.build.log {
        // FIXME: DRY default value. Do this inside `cli` once it uses the builder pattern.
        command.env("RUSTDOC_LOG", filter.as_deref().unwrap_or("debug"));
    }

    command.execute()
}

pub(crate) fn open(crate_name: CrateNameRef<'_>, flags: &cli::ProgramFlags) -> Result {
    let path = std::env::current_dir()?.join("doc").join(crate_name.as_str()).join("index.html");

    if flags.verbose {
        let verb = match flags.dry_run {
            false => "running",
            true => "skipping",
        };

        info(format!(
            "{verb} {} {}",
            "⟨browser⟩".color(palette::COMMAND).bold(),
            path.to_string_lossy().green()
        ))
        .emit();
    }

    if !flags.dry_run {
        open::that(path)?;
    }

    Ok(())
}

struct Command<'a> {
    command: process::Command,
    flags: &'a cli::ProgramFlags,
    strictness: Strictness,
    uses_unstable_options: bool,
}

impl<'a> Command<'a> {
    fn new(
        program: impl AsRef<OsStr>,
        flags: &'a cli::ProgramFlags,
        strictness: Strictness,
    ) -> Self {
        Self {
            command: process::Command::new(program),
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

        let verb = if !self.flags.dry_run { "running" } else { "skipping" };
        let mut message = String::from(verb);
        message += " ";
        self.render_into(&mut message).unwrap();

        info(message).emit();
    }

    fn set_toolchain(&mut self, flags: Flags<'_>) {
        if let Some(toolchain) = flags.toolchain {
            self.arg(toolchain);
        }
    }

    fn set_crate_name(&mut self, crate_name: CrateNameRef<'_>, path: InputPath<'_>) {
        if let Ok(fiducial_crate_name) = CrateNameCow::parse_from_input_path(path)
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
        if edition == default() {
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

    fn set_internals_mode(&mut self, flags: &cli::BuildFlags) {
        if flags.rustc_verbose_internals {
            self.arg("-Zverbose-internals");
        }
    }

    fn set_backtrace_behavior(&mut self, flags: &cli::BuildFlags) {
        if flags.no_backtrace {
            self.env("RUST_BACKTRACE", "0");
        }
    }

    fn set_cfgs(&mut self, flags: &cli::BuildFlags) {
        for cfg in &flags.cfgs {
            self.arg("--cfg");
            self.arg(cfg);
        }
        for feature in &flags.cargo_features {
            // FIXME: Warn on conflicts with `cfgs` from `self.arguments.cfgs`.
            self.arg("--cfg");
            self.arg(format!("feature=\"{feature}\""));
        }
    }

    fn set_rustc_features(&mut self, flags: &cli::BuildFlags) {
        for feature in &flags.rustc_features {
            self.arg(format!("-Zcrate-attr=feature({feature})"));
        }
    }

    fn set_cap_lints(&mut self, flags: &cli::BuildFlags) {
        if let Some(level) = &flags.cap_lints {
            self.arg("--cap-lints");
            self.arg(level);
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
        &self.command
    }
}

impl DerefMut for Command<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.command
    }
}

trait CommandExt {
    fn render_into(&self, buffer: &mut String) -> fmt::Result;
}

// This is very close to `<process::Command as fmt::Debug>::fmt` but prettier.
impl CommandExt for process::Command {
    fn render_into(&self, buffer: &mut String) -> fmt::Result {
        use std::fmt::Write;

        for (key, value) in self.get_envs() {
            // FIXME: Print `env -u VAR` for removed vars before
            // added vars just like `Command`'s `Debug` impl.
            let Some(value) = value else { continue };

            write!(
                buffer,
                "{}{}{} ",
                key.to_string_lossy().color(palette::VARIABLE).bold(),
                "=".color(palette::VARIABLE),
                value.to_string_lossy().color(palette::VARIABLE)
            )?;
        }

        write!(buffer, "{}", self.get_program().to_string_lossy().color(palette::COMMAND).bold())?;

        for argument in self.get_args() {
            write!(buffer, " {}", argument.to_string_lossy().color(palette::ARGUMENT))?;
        }

        Ok(())
    }
}

mod palette {
    use owo_colors::AnsiColors;

    pub(super) const VARIABLE: AnsiColors = AnsiColors::Yellow;
    pub(super) const COMMAND: AnsiColors = AnsiColors::Magenta;
    pub(super) const ARGUMENT: AnsiColors = AnsiColors::Green;
}

#[derive(Clone)]
pub(crate) enum ExternCrate<'src> {
    Unnamed { path: &'src str },
    Named { name: CrateNameRef<'src>, path: Option<Cow<'src, str>> },
}

#[derive(Clone, Copy)]
pub(crate) struct Flags<'a> {
    pub(crate) toolchain: Option<&'a OsStr>,
    pub(crate) build: &'a cli::BuildFlags,
    pub(crate) verbatim: VerbatimFlags<'a>,
    pub(crate) program: &'a cli::ProgramFlags,
}

#[derive(Clone, Copy)]
pub(crate) struct VerbatimFlags<'a> {
    pub(crate) arguments: &'a [&'a str],
    pub(crate) environment: &'a [(&'a str, Option<&'a str>)],
}

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
