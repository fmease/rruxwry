//! Low-level build commands.
//!
//! The high-level build commands are defined in [`crate::builder`].

// Note that we try to avoid generating unnecessary flags where possible even if that means
// doing more work on our side. The main motivation for this is being able to just copy/paste
// the commands printed by `--verbose` for use in GitHub discussions without requiring any
// manual minimization.
// FIXME: Also mention to reduce conflicts with compile flags passed via `compiletest`

use crate::{
    cli,
    error::Result,
    utility::{default, Tag},
};
use owo_colors::OwoColorize;
use std::{
    borrow::Cow,
    ffi::OsStr,
    fmt,
    ops::{Deref, DerefMut},
    path::Path,
    process,
    str::FromStr,
};

pub(crate) fn compile(
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    extern_crates: &[ExternCrate<'_>],
    build_flags: &cli::BuildFlags,
    program_flags: &cli::ProgramFlags,
    verbatim_flags: VerbatimFlags<'_>,
    strictness: Strictness,
) -> Result {
    let mut command = Command::new("rustc", program_flags, strictness);

    command.set_env_vars(build_flags);
    command.set_toolchain(build_flags);

    command.arg(path);

    command.set_edition(edition);
    command.set_crate_type(crate_type);
    command.set_crate_name(crate_name, path);

    command.set_extern_crates(extern_crates);

    command.set_cfgs(build_flags);
    command.set_rustc_features(build_flags);
    command.set_cap_lints(build_flags);
    command.set_internals_mode(build_flags);

    command.set_verbatim_flags(verbatim_flags);
    command.execute()
}

pub(crate) fn document(
    path: &Path,
    crate_name: CrateNameRef<'_>,
    crate_type: CrateType,
    edition: Edition,
    extern_crates: &[ExternCrate<'_>],
    build_flags: &cli::BuildFlags,
    program_flags: &cli::ProgramFlags,
    verbatim_flags: VerbatimFlags<'_>,
    strictness: Strictness,
) -> Result {
    let mut command = Command::new("rustdoc", program_flags, strictness);

    command.set_env_vars(build_flags);
    command.set_toolchain(build_flags);

    command.arg(path.as_os_str());

    command.set_crate_name(crate_name, path);
    if crate_type != default() {
        command.set_crate_type(crate_type);
    }
    command.set_edition(edition);

    command.set_extern_crates(extern_crates);

    if build_flags.json {
        command.arg("--output-format");
        command.arg("json");
        command.uses_unstable_options = true;
    }

    if build_flags.private {
        command.arg("--document-private-items");
    }

    if build_flags.hidden {
        command.arg("--document-hidden-items");
        command.uses_unstable_options = true;
    }

    if build_flags.layout {
        command.arg("--show-type-layout");
        command.uses_unstable_options = true;
    }

    if build_flags.link_to_definition {
        command.arg("--generate-link-to-definition");
        command.uses_unstable_options = true;
    }

    if build_flags.normalize {
        command.arg("-Znormalize-docs");
    }

    if let Some(crate_version) = &build_flags.crate_version {
        command.arg("--crate-version");
        command.arg(crate_version);
    }

    command.arg("--default-theme");
    command.arg(&build_flags.theme);

    command.set_cfgs(build_flags);
    command.set_rustc_features(build_flags);
    command.set_cap_lints(build_flags);
    command.set_internals_mode(build_flags);

    command.set_verbatim_flags(verbatim_flags);
    command.execute()
}

pub(crate) fn open(crate_name: CrateNameRef<'_>, flags: &cli::ProgramFlags) -> Result {
    let path = std::env::current_dir()?
        .join("doc")
        .join(crate_name.as_str())
        .join("index.html");

    if flags.verbose {
        eprint!("{}", Tag::Note);

        let title = match flags.dry_run {
            false => "opening",
            true => "skipping opening", // FIXME: awkward wording!
        };

        eprintln!("{title} {}", path.to_string_lossy().green());
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

        eprint!("{}", Tag::Note);

        let title = if !self.flags.dry_run {
            "running"
        } else {
            "skipping"
        };

        eprint!("{title} ");

        for (var, value) in self.get_envs() {
            // FIXME: Print `env -u VAR` for removed vars before
            // added vars just like `Command`'s `Debug` impl.
            let Some(value) = value else { continue };

            eprint!(
                "{}{}{} ",
                var.to_string_lossy().yellow().bold(),
                "=".yellow(),
                value.to_string_lossy().yellow()
            );
        }
        eprint!("{}", self.get_program().to_string_lossy().purple().bold());
        for arg in self.get_args() {
            eprint!(" {}", arg.to_string_lossy().green());
        }
        eprintln!();
    }

    fn set_toolchain(&mut self, flags: &cli::BuildFlags) {
        if let Some(toolchain) = &flags.toolchain {
            self.arg(format!("+{toolchain}"));
        }
    }

    fn set_crate_name(&mut self, crate_name: CrateNameRef<'_>, path: &Path) {
        // FIXME: unwrap
        let fiducial_crate_name = CrateName::from_path(path).unwrap();

        if crate_name != fiducial_crate_name.as_ref() {
            self.arg("--crate-name");
            self.arg(crate_name.as_str());
        }
    }

    fn set_crate_type(&mut self, crate_type: CrateType) {
        self.arg("--crate-type");
        self.arg(crate_type.to_str());
    }

    fn set_edition(&mut self, edition: Edition) {
        if edition != default() {
            self.arg("--edition");
            self.arg(edition.to_str());
        }
        if !edition.is_stable() {
            self.uses_unstable_options = true;
        }
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

    fn set_env_vars(&mut self, flags: &cli::BuildFlags) {
        if flags.log {
            self.env("RUSTC_LOG", "debug");
        }
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
        if let Some(level) = flags.cap_lints {
            self.arg("--cap-lints");
            self.arg(level.to_str());
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
        self.envs(flags.rustc_envs.iter().copied());
        for key in flags.unset_rustc_env {
            self.env_remove(key);
        }
        self.args(flags.compile_flags);
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

#[derive(Clone)]
pub(crate) enum ExternCrate<'src> {
    Unnamed {
        path: &'src str,
    },
    Named {
        name: CrateNameRef<'src>,
        path: Option<Cow<'src, str>>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) enum Edition {
    #[default]
    Edition2015,
    Edition2018,
    Edition2021,
    Edition2024,
}

impl Edition {
    pub(crate) const LATEST_STABLE: Self = Self::Edition2021;

    pub(crate) fn is_stable(self) -> bool {
        self <= Self::LATEST_STABLE
    }

    pub(crate) const fn to_str(self) -> &'static str {
        match self {
            Self::Edition2015 => "2015",
            Self::Edition2018 => "2018",
            Self::Edition2021 => "2021",
            Self::Edition2024 => "2024",
        }
    }

    // FIXME: Derive this.
    pub(crate) const fn elements() -> &'static [Self] {
        &[
            Self::Edition2015,
            Self::Edition2018,
            Self::Edition2021,
            Self::Edition2024,
        ]
    }
}

impl FromStr for Edition {
    type Err = ();

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Ok(match source {
            "2015" => Self::Edition2015,
            "2018" => Self::Edition2018,
            "2021" => Self::Edition2021,
            "2024" => Self::Edition2024,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(test, derive(Debug))]
pub(crate) enum CrateType {
    #[default]
    Bin,
    Lib,
    ProcMacro,
}

impl CrateType {
    pub(crate) const fn to_str(self) -> &'static str {
        match self {
            Self::Bin => "bin",
            Self::Lib => "lib",
            Self::ProcMacro => "proc-macro",
        }
    }

    pub(crate) fn crates(self) -> &'static [ExternCrate<'static>] {
        match self {
            // For convenience and just like Cargo we add `libproc_macro` to the external prelude.
            Self::ProcMacro => &[ExternCrate::Named {
                name: CrateName("proc_macro"),
                path: None,
            }],
            _ => [].as_slice(),
        }
    }

    pub(crate) fn to_non_executable(self) -> Self {
        match self {
            Self::Bin => Self::Lib,
            Self::Lib | Self::ProcMacro => self,
        }
    }
}

impl FromStr for CrateType {
    type Err = ();

    // FIXME: Support `dylib`, `staticlib` etc.
    fn from_str(source: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match source {
            "bin" => Self::Bin,
            "lib" | "rlib" => Self::Lib,
            "proc-macro" => Self::ProcMacro,
            _ => return Err(()),
        })
    }
}

pub(crate) type CrateNameBuf = CrateName<String>;
pub(crate) type CrateNameRef<'a> = CrateName<&'a str>;
pub(crate) type CrateNameCow<'a> = CrateName<Cow<'a, str>>;

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct CrateName<T: AsRef<str>>(T);

impl<T: AsRef<str>> CrateName<T> {
    pub(crate) fn new(name: T) -> Self {
        Self(name)
    }

    pub(crate) fn map<U: AsRef<str>>(self, mapper: impl FnOnce(T) -> U) -> CrateName<U> {
        CrateName(mapper(self.0))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl<'src> CrateNameRef<'src> {
    pub(crate) fn parse_strict(source: &'src str) -> Result<Self, ()> {
        let mut chars = source.chars();
        if let Some(char) = chars.next()
            && (char.is_ascii_alphabetic() || char == '_')
            && chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
        {
            Ok(CrateName::new(source))
        } else {
            Err(())
        }
    }
}

impl CrateNameBuf {
    pub(crate) fn from_path(path: &Path) -> Result<Self, ()> {
        // FIXME: This doesn't do any extra validation steps.
        path.file_stem()
            .and_then(|name| name.to_str())
            .map(|name| Self(name.replace('-', "_")))
            .ok_or(())
    }

    pub(crate) fn parse_lenient(source: &str) -> Result<Self, ()> {
        let mut chars = source.chars();
        if let Some(char) = chars.next()
            && (char.is_ascii_alphabetic() || char == '_' || char == '-')
            && chars.all(|char| char.is_ascii_alphanumeric() || char == '_' || char == '-')
        {
            let crate_name = source.replace('-', "_");
            Ok(CrateName::new(crate_name))
        } else {
            Err(())
        }
    }
}

impl<T: AsRef<str>> CrateName<T> {
    pub(crate) fn as_ref(&self) -> CrateNameRef<'_> {
        CrateName(self.0.as_ref())
    }
}

impl From<CrateNameBuf> for CrateNameCow<'_> {
    fn from(name: CrateNameBuf) -> Self {
        name.map(Cow::Owned)
    }
}

impl<'a> From<CrateNameRef<'a>> for CrateNameCow<'a> {
    fn from(name: CrateNameRef<'a>) -> Self {
        name.map(Cow::Borrowed)
    }
}

impl<T: AsRef<str>> fmt::Display for CrateName<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy)]
pub(crate) enum LintLevel {
    Allow,
    Warn,
    Deny,
    Forbid,
}

impl LintLevel {
    pub(crate) const fn to_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Deny => "deny",
            Self::Forbid => "forbid",
        }
    }

    // FIXME: Derive this.
    pub(crate) const fn elements() -> &'static [Self] {
        &[Self::Allow, Self::Warn, Self::Deny, Self::Forbid]
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct VerbatimFlags<'a> {
    pub(crate) compile_flags: &'a [&'a str],
    pub(crate) rustc_envs: &'a [(&'a str, &'a str)],
    pub(crate) unset_rustc_env: &'a [&'a str],
}

pub(crate) enum Strictness {
    Strict,
    Lenient,
}