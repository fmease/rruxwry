use std::fmt;

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

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Process(error) => error.fmt(f),
            Self::Build(error) => error.fmt(f),
        }
    }
}
