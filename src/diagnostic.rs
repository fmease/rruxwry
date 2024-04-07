use crate::utility::Str;
use owo_colors::{AnsiColors, OwoColorize};
use std::fmt;

pub(crate) fn error(message: impl Into<Str>) -> Diagnostic {
    Diagnostic::new(Severity::Error, message)
}

pub(crate) fn warning(message: impl Into<Str>) -> Diagnostic {
    Diagnostic::new(Severity::Warning, message)
}

pub(crate) fn info(message: impl Into<Str>) -> Diagnostic {
    Diagnostic::new(Severity::Info, message)
}

/// Just like [`Into<Diagnostic>`] but leads to nicer call sites.
pub(crate) trait IntoDiagnostic {
    fn into_diagnostic(self) -> Diagnostic;
}

pub(crate) struct Diagnostic {
    severity: Severity,
    message: Str,
    notes: Vec<Str>,
}

impl Diagnostic {
    fn new(severity: Severity, message: impl Into<Str>) -> Self {
        Self {
            severity,
            message: message.into(),
            notes: Vec::new(),
        }
    }

    pub(crate) fn note(mut self, note: impl Into<Str>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub(crate) fn emit(self) {
        eprintln!("{self}");
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.severity, self.message)?;

        if !self.notes.is_empty() {
            writeln!(f)?;

            let padding = self.severity.name().len() + ": ".len();
            let padding = " ".repeat(padding);

            let mut first = true;
            for note in &self.notes {
                if !first {
                    writeln!(f)?;
                } else {
                    first = false;
                }
                write!(f, "{padding}{Note}: {note}")?;
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    const fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }

    const fn color(self) -> AnsiColors {
        match self {
            Self::Info => AnsiColors::Cyan,
            Self::Warning => AnsiColors::Yellow,
            Self::Error => AnsiColors::Red,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name().color(Self::color(*self)))
    }
}

struct Note;

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", "note".blue())
    }
}
