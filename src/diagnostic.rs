use crate::{
    context::Context,
    source::{LocalSpan, Span},
    utility,
};
use anstream::ColorChoice;
use anstyle::{AnsiColor, Effects};
use std::io::{self, Write as _};
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

pub(crate) type Painter = utility::paint::Painter<io::BufWriter<io::StderrLock<'static>>>;

pub(crate) fn bug(message: impl FnOnce(&mut Painter) -> io::Result<()>) -> Diagnostic {
    Diagnostic::new(Severity::Bug, message)
}

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
    // FIXME: Actually take the painter as a parameter!!! right now we're
    //        constructing a new painter for each `emit!()` which is semi-expensive!
    // Update: Obtain the painter from `cx: Content<'_>` once that contains one.
    //  NOTE: if we do that change, don't keep the lock the entire time!
    //        we want rustc to print to stderr too!
    fn new(severity: Severity, message: impl FnOnce(&mut Painter) -> io::Result<()>) -> Self {
        let stderr = io::stderr().lock();
        let colorize = anstream::AutoStream::choice(&stderr) != ColorChoice::Never;
        let mut p = Painter::new(io::BufWriter::new(stderr), colorize);
        (|| {
            p.set(Effects::BOLD)?;
            p.with(severity.color(), |p| write!(p, "[rruxwry] {}", severity.name()))?;
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

    pub(crate) fn highlight(mut self, span: Span, cx: Context<'_>) -> Self {
        let file = cx.map().by_span(span);
        let span = span.local(file);
        let (line_number, line, span) = resolve(file.contents, span);
        let column_number = line[..span.start as usize].graphemes(true).count() + 1;
        let underline_offset = line[..span.start as usize].width();
        let underline_width = line[span.range()].width();

        let p = &mut self.p;
        (|| {
            writeln!(p, "   {}:{line_number}:{column_number}", file.path.display())?;

            writeln!(p, "{line}")?;

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

    // FIXME: Split message by line and properly offset each resulting line.
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
    Bug,
    Error,
    Warning,
    Debug,
}

impl Severity {
    const fn name(self) -> &'static str {
        match self {
            Self::Bug => "internal error",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Debug => "",
        }
    }

    const fn color(self) -> AnsiColor {
        match self {
            Self::Bug | Self::Error => AnsiColor::BrightRed,
            Self::Warning => AnsiColor::BrightYellow,
            Self::Debug => AnsiColor::BrightBlue,
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

fn resolve(source: &str, span: LocalSpan) -> (usize, &str, LocalSpan) {
    let needle = &source[span.range()];

    // We assume that hightlights only span a single line.
    for (index, line) in source.split('\n').enumerate() {
        if let Some(range) = line.substr_range(needle) {
            return (index + 1, line, LocalSpan {
                start: range.start.try_into().unwrap(),
                end: range.end.try_into().unwrap(),
            });
        }
    }

    unreachable!()
}
