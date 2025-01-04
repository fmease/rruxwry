use crate::diagnostic::{EmittedError, error, fmt};
use std::{io, process};

pub(crate) type Result<T = (), E = Error> = std::result::Result<T, E>;

pub(crate) enum Error {
    Io(io::Error),
    Process(process::ExitStatusError),
    Emitted(EmittedError),
}

impl Error {
    pub(crate) fn emit(self) {
        match self {
            Self::Io(error) => self::error(fmt!("{error}")).finish(),
            Self::Process(error) => self::error(fmt!("{error}")).finish(),
            Self::Emitted(error) => error,
        };
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<process::ExitStatusError> for Error {
    fn from(error: process::ExitStatusError) -> Self {
        Self::Process(error)
    }
}

impl From<EmittedError> for Error {
    fn from(error: EmittedError) -> Self {
        Self::Emitted(error)
    }
}
