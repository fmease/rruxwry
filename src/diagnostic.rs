use crate::utility;
use anstream::ColorChoice;
use anstyle::AnsiColor;
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
        let stderr = io::stderr().lock();
        let colorize = anstream::AutoStream::choice(&stderr) != ColorChoice::Never;
        let mut p = Painter::new(io::BufWriter::new(stderr), colorize);
        severity.paint(&mut p).unwrap();
        write!(&mut p, ": ").unwrap();
        message(&mut p).unwrap();
        let padding = " ".repeat(severity.name().len() + ": ".len());
        Self { p, padding }
    }

    pub(crate) fn note(mut self, note: impl FnOnce(&mut Painter) -> io::Result<()>) -> Self {
        writeln!(&mut self.p).unwrap();
        // FIXME: can we use one of the format modifiers to allow us to store padding as usize?
        write!(&mut self.p, "{}", self.padding).unwrap();
        Severity::Note.paint(&mut self.p).unwrap();
        write!(&mut self.p, ": ").unwrap();
        note(&mut self.p).unwrap();
        self
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

    fn paint(self, p: &mut Painter) -> io::Result<()> {
        p.with(self.color(), |p| write!(p, "{}", self.name()))
    }
}
