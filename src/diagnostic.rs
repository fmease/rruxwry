use crate::utility;
use anstream::ColorChoice;
use anstyle::{AnsiColor, Effects};
use std::io::{self, Write};

pub(crate) type Painter = utility::paint::Painter<io::BufWriter<io::StderrLock<'static>>>;

pub(crate) struct Emitter {
    p: Painter,
    padding: String,
}

impl Emitter {
    // FIXME: actually take the painter as a parameter!!! right now we're
    //        constructing a new painter for each `emit!()`!
    //  NOTE: if we do that change, don't keep the lock the entire time!
    //        we want rustc to print to stderr too!
    pub(crate) fn new(
        severity: Severity,
        message: impl FnOnce(&mut Painter) -> io::Result<()>,
    ) -> Self {
        Self::try_new(severity, message).unwrap()
    }

    fn try_new(
        severity: Severity,
        message: impl FnOnce(&mut Painter) -> io::Result<()>,
    ) -> io::Result<Self> {
        let stderr = io::stderr().lock();
        let colorize = anstream::AutoStream::choice(&stderr) != ColorChoice::Never;
        let mut p = Painter::new(io::BufWriter::new(stderr), colorize);
        p.set(Effects::BOLD)?;
        p.with(severity.color(), |p| write!(p, "{}[rruxwry]", severity.name()))?;
        write!(&mut p, ": ")?;
        message(&mut p)?;
        p.unset()?;
        let padding = " ".repeat(severity.name().len() + ": ".len());
        Ok(Self { p, padding })
    }

    pub(crate) fn note(self, note: impl FnOnce(&mut Painter) -> io::Result<()>) -> Self {
        self.try_note(note).unwrap()
    }

    fn try_note(mut self, note: impl FnOnce(&mut Painter) -> io::Result<()>) -> io::Result<Self> {
        writeln!(self.p)?;
        // FIXME: can we use one of the format modifiers to allow us to store padding as usize?
        write!(self.p, "{}", self.padding)?;
        self.p.with(Severity::Note.color(), |p| write!(p, "{}", Severity::Note.name()))?;
        write!(self.p, ": ")?;
        note(&mut self.p)?;
        Ok(self)
    }

    pub(crate) fn finish(mut self) -> EmittedError {
        writeln!(self.p).unwrap();
        EmittedError(())
    }
}

pub(crate) struct EmittedError(());

pub(crate) macro emit {
    ($Severity:ident($( $message:tt )*) $( .note( $( $note:tt )* ) )*) => {
        Emitter::new(Severity::$Severity, painter!($( $message )*))
            $( .note(painter!($( $note )*)) )*
            .finish()
    },
}

macro painter {
    (|$p:ident| $e:expr) => { |$p| $e },
    ($( $arg:tt )*) => { |p| write!(p, $( $arg )*) },
}

#[derive(Clone, Copy)]
pub(crate) enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    const fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        }
    }

    const fn color(self) -> AnsiColor {
        match self {
            Self::Error => AnsiColor::Red,
            Self::Warning => AnsiColor::Yellow,
            Self::Note => AnsiColor::Blue,
        }
    }
}
