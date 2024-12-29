use crate::diagnostic::error;

pub(crate) type Result<T = (), E = Error> = std::result::Result<T, E>;

pub(crate) enum Error {
    Io(std::io::Error),
    Process(std::process::ExitStatusError),
    Build(Box<crate::operate::Error>),
}

impl Error {
    pub(crate) fn emit(self) {
        match self {
            Self::Io(error) => self::error!("{error}"),
            Self::Process(error) => self::error!("{error}"),
            Self::Build(error) => error.emit(),
        }
    }
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

impl From<crate::operate::Error> for Error {
    fn from(error: crate::operate::Error) -> Self {
        Self::Build(Box::new(error))
    }
}
