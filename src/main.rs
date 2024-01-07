#![feature(let_chains)]

use clap::Parser;
use owo_colors::OwoColorize;
use std::{
    borrow::Cow,
    io,
    path::PathBuf,
    process::{Command, ExitCode, ExitStatus},
};

const EDITION: &str = "2021";

// FIXME: Support `--crate-name=dependency/dependent`.
// FIXME: Support passing additional arguments verbatim to `rustc` & `rustdoc`.
// FIXME: Support non-auto-generated dependents (via `-x=path/to/file.rs`).
// FIXME: Support convenience flag for setting the CSS theme.

#[derive(Parser)]
#[command(about)]
struct Arguments {
    /// Path to the source file.
    path: PathBuf,
    /// Set the edition of the source files.
    #[arg(short, long, default_value = EDITION)]
    edition: String,
    /// Document hidden items.
    #[arg(short = 'H', long)]
    hidden: bool,
    /// Output JSON instead of HTML.
    #[arg(short, long)]
    json: bool,
    /// Document the memory layout of types.
    #[arg(short = 'L', long)]
    layout: bool,
    /// Normalize types and constants.
    #[arg(short = 'N', long)]
    normalize: bool,
    /// Set the (base) name of the crate(s).
    #[arg(short = 'n', long, value_name("NAME"))]
    crate_name: Option<String>,
    /// Set the version of the (root) crate.
    #[arg(short = 'v', long, value_name("VERSION"))]
    crate_version: Option<String>,
    /// Open the generated docs in a browser.
    #[arg(short, long)]
    open: bool,
    /// Document private items.
    #[arg(short = 'P', long)]
    private: bool,
    /// Pick up the crate name from `#![crate_name]` if available.
    #[arg(short = 'a', long)]
    crate_name_attr: bool,
    /// Set the toolchain.
    #[arg(short, long)]
    toolchain: Option<String>,
    /// Use verbose output.
    #[arg(short = 'V', long)]
    verbose: bool,
    /// Enable rustc's `-Zverbose`.
    #[arg(short = 'W', long)]
    rustc_verbose: bool,
    /// Override `RUSTC_LOG` to be `debug`.
    #[arg(short, long)]
    log: bool,
    /// Override `RUST_BACKTRACE` to be `0`.
    #[arg(short = 'B', long)]
    no_backtrace: bool,
    /// Enable the cross-crate re-export mode.
    #[arg(short = 'x', long)]
    cross_crate: bool,
}

struct Application {
    arguments: Arguments,
    crate_name: String,
    dependent_crate_name: Option<String>,
}

impl Application {
    fn new() -> Self {
        let mut arguments = Arguments::parse();

        let mut crate_name_from_attribute = None;

        // Look for `#![crate_name]` in a naive fashion.
        if let Ok(file) = std::fs::read_to_string(&arguments.path)
            && let Some(line) = file
                .lines()
                .find_map(|line| line.strip_prefix("#![crate_name = \""))
            && let Some(crate_name) = line.strip_suffix("\"]")
        {
            if arguments.crate_name_attr {
                crate_name_from_attribute = Some(crate_name.into());
            } else {
                warning();
                eprintln!(
                    "ignoring potential `#![crate_name]` attribute (setting crate name to `{crate_name}`); \
                     pass `-a` / `--crate-name-attr` to pick it up"
                );
            }
        }

        let crate_name = match (arguments.crate_name.as_deref(), crate_name_from_attribute) {
            (Some(crate_name), Some(crate_name_from_attribute)) => {
                warning();
                eprintln!(
                    "dropping crate name `{crate_name}` from program arguments \
                     in favor of `{crate_name_from_attribute}` from `#![crate_name]`"
                );

                arguments.crate_name = None;
                crate_name_from_attribute
            }
            (Some(crate_name), None) => crate_name.into(),
            (None, Some(crate_name)) => {
                note();
                eprintln!("using crate name `{crate_name}` from `#![crate_name]`");

                crate_name
            }
            (None, None) => arguments
                .path
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .replace('-', "_"),
        };

        let dependent_crate_name = arguments.cross_crate.then(|| format!("u_{crate_name}"));

        Self {
            arguments,
            crate_name,
            dependent_crate_name,
        }
    }

    fn compile(&self) -> io::Result<ExitStatus> {
        if !self.arguments.cross_crate {
            return Ok(ExitStatus::default());
        }

        let mut command = Command::new("rustc");

        if self.arguments.log {
            command.env("RUSTC_LOG", "debug");
        }

        if self.arguments.no_backtrace {
            command.env("RUST_BACKTRACE", "0");
        }

        if let Some(toolchain) = &self.arguments.toolchain {
            command.arg(format!("+{}", expand_toolchain(toolchain)));
        }

        command.arg(&self.arguments.path);

        command.arg("--crate-type=lib");
        command.arg("--edition");
        command.arg(&self.arguments.edition);

        if let Some(crate_name) = &self.arguments.crate_name {
            command.arg("--crate-name");
            command.arg(crate_name);
        }

        if self.arguments.rustc_verbose {
            command.arg("-Zverbose");
        }

        if self.arguments.verbose {
            note();
            eprintln!("running: {command:?}");
        }

        command.status()
    }

    // @Task support passing additional arguments verbatim to rustdoc
    fn document(&self) -> io::Result<ExitStatus> {
        let mut command = Command::new("rustdoc");
        let mut uses_unstable_options = false;

        if self.arguments.log {
            command.env("RUSTC_LOG", "debug");
        }

        if self.arguments.no_backtrace {
            command.env("RUST_BACKTRACE", "0");
        }

        if let Some(toolchain) = &self.arguments.toolchain {
            command.arg(format!("+{}", expand_toolchain(toolchain)));
        }

        let path: Cow<'_, _> = if let Some(dependent_crate_name) = &self.dependent_crate_name {
            let path = self
                .arguments
                .path
                .with_file_name(dependent_crate_name)
                .with_extension("rs");

            if !path.exists() {
                std::fs::write(&path, format!("pub use {}::*;\n", self.crate_name))?;
            }

            path.into()
        } else {
            (&self.arguments.path).into()
        };

        command.arg(path.as_os_str());

        command.arg("--edition");
        command.arg(&self.arguments.edition);

        if self.arguments.json {
            command.arg("--output-format");
            command.arg("json");
            uses_unstable_options = true;
        }

        if self.arguments.private {
            command.arg("--document-private-items");
        }

        if self.arguments.hidden {
            command.arg("--document-hidden-items");
            uses_unstable_options = true;
        }

        if self.arguments.layout {
            command.arg("--show-type-layout");
            uses_unstable_options = true;
        }

        if self.arguments.normalize {
            command.arg("-Znormalize-docs");
        }

        if self.arguments.crate_name.is_some() {
            command.arg("--crate-name");
            command.arg(
                self.dependent_crate_name
                    .as_ref()
                    .unwrap_or(&self.crate_name),
            );
        }

        if let Some(crate_version) = &self.arguments.crate_version {
            command.arg("--crate-version");
            command.arg(crate_version);
        }

        if self.arguments.cross_crate {
            command.arg("--extern");
            command.arg(format!("{0}=lib{0}.rlib", self.crate_name));
        }

        if uses_unstable_options {
            command.arg("-Zunstable-options");
        }

        if self.arguments.rustc_verbose {
            command.arg("-Zverbose");
        }

        if self.arguments.verbose {
            note();
            eprintln!("running: {command:?}");
        }

        command.status()
    }

    fn open(&self) -> io::Result<()> {
        if !self.arguments.open {
            return Ok(());
        }

        let path = std::env::current_dir()?
            .join("doc")
            .join(
                self.dependent_crate_name
                    .as_ref()
                    .unwrap_or(&self.crate_name),
            )
            .join("index.html");

        if self.arguments.verbose {
            note();
            eprintln!("opening: {}", path.to_string_lossy());
        }

        open::that(path)
    }
}

fn expand_toolchain(name: &str) -> &str {
    match name {
        "S" => "stable",
        "B" => "beta",
        "N" => "nightly",
        "1" => "stage1",
        "2" => "stage2",
        "21" => "2stage1",
        "22" => "2stage2",
        name => name,
    }
}

fn note() {
    eprint!("{}: ", "note".cyan());
}

fn warning() {
    eprint!("{}: ", "warning".yellow());
}

fn main() -> io::Result<ExitCode> {
    let application = Application::new();

    application.compile()?;
    application.document()?;
    application.open()?;

    Ok(ExitCode::SUCCESS)
}
