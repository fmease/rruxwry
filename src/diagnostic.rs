use crate::utility;
use anstream::ColorChoice;
use anstyle::{AnsiColor, Effects};
use std::{
    io::{self, Write},
    path::Path,
};
use unicode_width::UnicodeWidthStr;

pub(crate) type Painter = utility::paint::Painter<io::BufWriter<io::StderrLock<'static>>>;

pub(crate) fn error(message: impl FnOnce(&mut Painter) -> io::Result<()>) -> Diagnostic {
    Diagnostic::new(Severity::Error, message)
}

pub(crate) fn warn(message: impl FnOnce(&mut Painter) -> io::Result<()>) -> Diagnostic {
    Diagnostic::new(Severity::Warning, message)
}

pub(crate) fn debug(message: impl FnOnce(&mut Painter) -> io::Result<()>) -> Diagnostic {
    Diagnostic::new(Severity::Debug, message)
}

pub(crate) macro fmt {
    ($( $arg:tt )*) => { |p| write!(p, $( $arg )*) },
}

#[must_use = "use `Diagnostic::finish` to complete the diagnostic"]
pub(crate) struct Diagnostic {
    p: Painter,
    aux_offset: Option<usize>,
    aux_seen: bool,
}

impl Diagnostic {
    // FIXME: actually take the painter as a parameter!!! right now we're
    //        constructing a new painter for each `emit!()` which is semi-expensive!
    //  NOTE: if we do that change, don't keep the lock the entire time!
    //        we want rustc to print to stderr too!
    pub(crate) fn new(
        severity: Severity,
        message: impl FnOnce(&mut Painter) -> io::Result<()>,
    ) -> Self {
        let stderr = io::stderr().lock();
        let colorize = anstream::AutoStream::choice(&stderr) != ColorChoice::Never;
        let mut p = Painter::new(io::BufWriter::new(stderr), colorize);
        (|| {
            p.set(Effects::BOLD)?;
            p.with(severity.color(), |p| write!(p, "{}[rruxwry]", severity.name()))?;
            if !severity.is_serious() {
                p.unset()?;
            }
            write!(&mut p, ": ")?;
            message(&mut p)?;
            if severity.is_serious() {
                p.unset()?;
            }
            io::Result::Ok(())
        })()
        .unwrap();
        Self { p, aux_offset: None, aux_seen: false }
    }

    pub(crate) fn location(mut self, location: Location<'_>) -> Self {
        let p = &mut self.p;
        (|| {
            writeln!(
                p,
                "   {}:{}:{}",
                location.path.display(),
                location.span.line + 1,
                // FIXME: column isn't measured in bytes but either in Unicode scalar
                // values or Unicode grapheme clusters, I don't remember.
                location.span.start + 1
            )?;

            // FIXME: Replace this with slicing instead (smh obtain the span of the line)
            let line = location.source.lines().nth(location.span.line as _).unwrap();
            writeln!(p, "{line}")?;

            let underline_offset = location.source[..location.span.start as usize].width();
            let underline_width =
                location.source[location.span.start as usize..location.span.end as usize].width();

            let (underline, underline_width) = match (underline_offset, underline_width) {
                (0, 0) => ("\\".into(), const { "\\".len() }),
                (_, 0) => ("/\\".into(), const { "/\\".len() }),
                (_, width) => ("^".repeat(width), width),
            };

            p.set(AnsiColor::BrightRed.on_default().bold())?;
            write!(p, "{}{underline}", " ".repeat(underline_offset),)?;
            p.unset()?;

            self.aux_offset = Some(underline_offset + underline_width);

            io::Result::Ok(())
        })()
        .unwrap();
        self
    }

    pub(crate) fn note(self, message: impl FnOnce(&mut Painter) -> io::Result<()>) -> Self {
        self.aux(AuxSeverity::Note, message)
    }

    pub(crate) fn help(self, message: impl FnOnce(&mut Painter) -> io::Result<()>) -> Self {
        self.aux(AuxSeverity::Help, message)
    }

    fn aux(
        mut self,
        severity: AuxSeverity,
        message: impl FnOnce(&mut Painter) -> io::Result<()>,
    ) -> Self {
        const DEFAULT_OFFSET: usize = 1;
        let p = &mut self.p;
        (|| {
            if self.aux_offset.is_none() || self.aux_seen {
                writeln!(p)?;
                write!(p, "{}", " ".repeat(self.aux_offset.unwrap_or(DEFAULT_OFFSET)))?;
            }
            p.with(AuxSeverity::COLOR.on_default().bold(), |p| write!(p, " {}", severity.name()))?;
            write!(p, ": ")?;
            message(p)
        })()
        .unwrap();
        self.aux_seen = true;
        self
    }

    pub(crate) fn finish(mut self) -> EmittedError {
        writeln!(self.p).unwrap();
        EmittedError(())
    }
}

pub(crate) struct EmittedError(());

#[derive(Clone, Copy)]
pub(crate) enum Severity {
    Error,
    Warning,
    Debug,
}

impl Severity {
    const fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Debug => "",
        }
    }

    const fn color(self) -> AnsiColor {
        match self {
            Self::Error => AnsiColor::BrightRed,
            Self::Warning => AnsiColor::Yellow,
            Self::Debug => AnsiColor::Blue,
        }
    }

    const fn is_serious(self) -> bool {
        matches!(self, Self::Error | Self::Warning)
    }
}

pub(crate) enum AuxSeverity {
    Note,
    Help,
}

impl AuxSeverity {
    const COLOR: AnsiColor = AnsiColor::BrightWhite;

    const fn name(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Help => "help",
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct Location<'a> {
    pub(crate) source: &'a str,
    pub(crate) path: &'a Path,
    pub(crate) span: LineSpan,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct LineSpan {
    pub(crate) line: u32,
    pub(crate) start: u32,
    pub(crate) end: u32,
}
