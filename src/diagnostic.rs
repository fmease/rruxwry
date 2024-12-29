use anstream::ColorChoice;
use anstyle::AnsiColor;
use std::io::{self, Write};

pub(crate) type Painter = crate::utility::paint::Painter<io::BufWriter<io::StderrLock<'static>>>;

// FIXME: (sub)notes smh
pub(crate) fn emit(severity: Severity, message: impl FnOnce(&mut Painter) -> io::Result<()>) {
    let stderr = io::stderr().lock();
    let colorize = anstream::AutoStream::choice(&stderr) != ColorChoice::Never;
    let p = &mut Painter::new(io::BufWriter::new(stderr), colorize);
    severity.paint(p).unwrap();
    write!(p, ": ").unwrap();
    message(p).unwrap();
    writeln!(p).unwrap();
}

// if !self.notes.is_empty() {
//     writeln!(p)?;

//     let padding = " ".repeat(self.severity.name().len() + ": ".len());

//     // FIXME: first is not necessary lol!!! look up!
//     let mut first = true;
//     for note in &self.notes {
//         if !first {
//             writeln!(p)?;
//         } else {
//             first = false;
//         }
//         write!(p, "{padding}")?;
//         Severity::Note.paint(p)?;
//         write!(p, ": {note}")?;
//     }
// }

pub(crate) macro error($( $arg:tt )*) { diag!(Error $( $arg )*) }
pub(crate) macro warning($( $arg:tt )*) { diag!(Warning $( $arg )*) }
pub(crate) macro note($( $arg:tt )*) { diag!(Note $( $arg )*) }

macro diag {
    ($severity:ident |$p:ident| $e:expr) => {
        emit(Severity::$severity, |$p| $e)
    },
    ($severity:ident $( $arg:tt )*) => {
        emit(Severity::$severity, |p| write!(p, $( $arg )*))
    }
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

    // FIXME: temp
    fn paint(&self, p: &mut Painter) -> io::Result<()> {
        p.with(self.color(), |p| write!(p, "{}", self.name()))
    }
}
