use super::palette;
use crate::{
    context::Context,
    diagnostic::{self, debug},
};
use anstyle::Effects;
use std::{
    ffi::OsStr,
    io::{self, Write as _},
    process,
};

pub(crate) struct Command {
    raw: process::Command,
}

impl Command {
    pub(super) fn new(program: impl AsRef<OsStr>) -> Self {
        Self { raw: process::Command::new(program) }
    }

    pub(super) fn arg(&mut self, arg: impl AsRef<OsStr>) {
        self.raw.arg(arg);
    }

    pub(super) fn args(&mut self, args: impl IntoIterator<Item: AsRef<OsStr>>) {
        self.raw.args(args);
    }

    pub(super) fn env(&mut self, key: impl AsRef<OsStr>, value: Option<impl AsRef<OsStr>>) {
        match value {
            Some(value) => self.raw.env(key, value),
            None => self.raw.env_remove(key),
        };
    }

    pub(crate) fn execute_capturing_output(
        mut self,
        cx: Context<'_>,
    ) -> io::Result<process::Output> {
        self.log(cx);
        self.raw.output()
    }

    pub(crate) fn execute(mut self, cx: Context<'_>) -> io::Result<process::ExitStatus> {
        self.log(cx);
        self.raw.status()
    }

    fn log(&self, cx: Context<'_>) {
        if cx.opts().dbg_opts.verbose {
            #[rustfmt::skip]
            debug(|p| { write!(p, "running ")?; self.paint(p) }).done();
        }
    }

    // This is very close to `<process::Command as fmt::Debug>::fmt` but prettier.
    // FIXME: This lacks shell escaping!
    fn paint(&self, p: &mut diagnostic::Painter) -> io::Result<()> {
        let envs = self.raw.get_envs();
        if !envs.is_empty() {
            p.set(palette::VARIABLE)?;
            for (key, value) in envs {
                // FIXME: Use more conventional "env -u..."
                if value.is_none() {
                    write!(p, "¬")?;
                }

                // FIXME: Escape key when need be.
                p.with(Effects::BOLD, |p| write!(p, "{}", key.display()))?;

                if let Some(value) = value {
                    write!(p, "={} ", value.display())?
                }
            }
            p.unset()?;
        }

        p.with(palette::COMMAND.on_default().bold(), |p| {
            write!(p, "{}", self.raw.get_program().display())
        })?;

        for arg in self.raw.get_args() {
            p.with(palette::ARGUMENT, |p| write!(p, " {}", arg.display()))?;
        }

        Ok(())
    }
}
