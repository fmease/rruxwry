use super::default;
use anstyle::{AnsiColor, Effects, Style};
use smallvec::SmallVec;
use std::io;

pub(crate) struct Painter<W: io::Write> {
    writer: W,
    colorize: bool,
    stack: SmallVec<Style, 1>,
}

impl<W: io::Write> Painter<W> {
    pub(crate) fn new(writer: W, colorize: bool) -> Self {
        Self { writer, colorize, stack: default() }
    }
}

impl<W: io::Write> Painter<W> {
    pub(crate) fn set(&mut self, style: impl IntoStyle) -> io::Result<()> {
        if !self.colorize {
            return Ok(());
        }

        let style = style.into_style();
        self.stack.push(style);
        style.write_to(&mut self.writer)
    }

    pub(crate) fn unset(&mut self) -> io::Result<()> {
        if !self.colorize {
            return Ok(());
        }

        if let Some(style) = self.stack.pop() {
            style.write_reset_to(&mut self.writer)?;
        }

        for style in &self.stack {
            style.write_to(&mut self.writer)?;
        }

        Ok(())
    }

    pub(crate) fn with(
        &mut self,
        style: impl IntoStyle,
        inner: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<()> {
        self.set(style)?;
        inner(self)?;
        self.unset()
    }
}

impl<W: io::Write> io::Write for Painter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.writer.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub(crate) trait IntoStyle {
    fn into_style(self) -> Style;
}

impl IntoStyle for Style {
    fn into_style(self) -> Style {
        self
    }
}

impl IntoStyle for AnsiColor {
    fn into_style(self) -> Style {
        self.on_default()
    }
}

impl IntoStyle for Effects {
    fn into_style(self) -> Style {
        Style::new().effects(self)
    }
}
