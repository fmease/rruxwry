use crate::diagnostic::{Diagnostic, IntoDiagnostic, error};

pub(crate) type Result<T = (), E = Error> = std::result::Result<T, E>;

pub(crate) enum Error {
    Io(std::io::Error),
    Process(std::process::ExitStatusError),
    Build(Box<crate::builder::Error>),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<std::process::ExitStatusError> for Error {
    fn from(error: std::process::ExitStatusError) -> Self {
        Self::Process(error)
    }
}

impl From<crate::builder::Error> for Error {
    fn from(error: crate::builder::Error) -> Self {
        Self::Build(Box::new(error))
    }
}

impl IntoDiagnostic for Error {
    fn into_diagnostic(self) -> Diagnostic {
        match self {
            Self::Io(error) => self::error(error.to_string()),
            Self::Process(error) => self::error(error.to_string()),
            Self::Build(error) => error.into_diagnostic(),
        }
    }
}
