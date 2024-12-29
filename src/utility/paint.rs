use super::{SmallVec, default};
use anstyle::{AnsiColor, Effects, Style};
use std::io::{self, BufWriter, StderrLock, StdoutLock, Write};

// FIXME: remove these again maybe
pub(crate) type BufStdoutPainter = Painter<BufWriter<StdoutLock<'static>>>;
pub(crate) type BufStderrPainter = Painter<BufWriter<StderrLock<'static>>>;

// pub(crate) fn paint(
//     paint: impl FnOnce(&mut BufStdoutPainter) -> io::Result<()>,
//     choice: ColorChoice,
// ) -> io::Result<()> {
//     let mut painter = Painter::stdout(choice);
//     paint(&mut painter)?;
//     painter.flush()
// }

// pub(crate) fn epaint(
//     paint: impl FnOnce(&mut BufStderrPainter) -> io::Result<()>,
//     choice: ColorChoice,
// ) -> io::Result<()> {
//     let mut painter = Painter::stderr(choice);
//     paint(&mut painter)?;
//     painter.flush()
// }

pub(crate) struct Painter<W: Write> {
    writer: W,
    colorize: bool,
    stack: SmallVec<Style, 1>,
}

impl<W: Write> Painter<W> {
    pub(crate) fn new(writer: W, colorize: bool) -> Self {
        Self { writer, colorize, stack: default() }
    }
}

impl<W: Write> Painter<W> {
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

impl<W: Write> Write for Painter<W> {
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

#[allow(dead_code)] // may come in clutch at some point
pub(crate) trait ColorExt {
    fn to_bg(self) -> Style;
}

impl ColorExt for AnsiColor {
    fn to_bg(self) -> Style {
        Style::new().bg_color(Some(self.into()))
    }
}
