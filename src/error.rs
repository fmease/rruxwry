use crate::diagnostic::{EmittedError, error, fmt};
use std::io;

pub(crate) type Result<T = (), E = Error> = std::result::Result<T, E>;

pub(crate) enum Error {
    Io(io::Error),
    Emitted(EmittedError),
}

impl Error {
    pub(crate) fn emit(self) -> EmittedError {
        match self {
            Self::Io(error) => self::error(fmt!("{error}")).done(),
            Self::Emitted(error) => error,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<EmittedError> for Error {
    fn from(error: EmittedError) -> Self {
        Self::Emitted(error)
    }
}
