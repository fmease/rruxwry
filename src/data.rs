use crate::utility::parse;
use std::{borrow::Cow, fmt, path::Path, str::FromStr};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Edition {
    Edition2015,
    Edition2018,
    Edition2021,
    Edition2024,
}

impl Edition {
    pub(crate) const RUSTC_DEFAULT: Self = Self::Edition2015;
    pub(crate) const LATEST_STABLE: Self = Self::Edition2024;
    pub(crate) const BLEEDING_EDGE: Self = Self::LATEST_STABLE;

    pub(crate) fn is_stable(self) -> bool {
        self <= Self::LATEST_STABLE
    }

    pub(crate) const fn to_str(self) -> &'static str {
        match self {
            Self::Edition2015 => "2015",
            Self::Edition2018 => "2018",
            Self::Edition2021 => "2021",
            Self::Edition2024 => "2024",
        }
    }
}

impl FromStr for Edition {
    type Err = ();

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Ok(match source {
            "2015" => Self::Edition2015,
            "2018" => Self::Edition2018,
            "2021" => Self::Edition2021,
            "2024" => Self::Edition2024,
            _ => return Err(()),
        })
    }
}

// FIXME: Support `dylib`, `staticlib`, etc.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(test, derive(Debug))]
pub(crate) enum CrateType {
    #[default]
    Bin,
    Lib,
    ProcMacro,
}

impl CrateType {
    pub(crate) const fn to_str(self) -> &'static str {
        match self {
            Self::Bin => "bin",
            Self::Lib => "lib",
            Self::ProcMacro => "proc-macro",
        }
    }

    pub(crate) fn to_non_executable(self) -> Self {
        match self {
            Self::Bin => Self::Lib,
            Self::Lib | Self::ProcMacro => self,
        }
    }
}

impl FromStr for CrateType {
    type Err = impl Iterator<Item = &'static str> + Clone;

    fn from_str(source: &str) -> std::result::Result<Self, Self::Err> {
        parse!(
            "bin" => Self::Bin,
            "lib" | "rlib" => Self::Lib,
            "proc-macro" => Self::ProcMacro,
        )(source)
    }
}

pub(crate) type CrateNameBuf = CrateName<String>;
pub(crate) type CrateNameRef<'a> = CrateName<&'a str>;
pub(crate) type CrateNameCow<'a> = CrateName<Cow<'a, str>>;

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct CrateName<T: AsRef<str>>(T);

impl<T: AsRef<str>> CrateName<T> {
    pub(crate) const fn new_unchecked(name: T) -> Self {
        Self(name)
    }

    pub(crate) fn map<U: AsRef<str>>(self, mapper: impl FnOnce(T) -> U) -> CrateName<U> {
        CrateName(mapper(self.0))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl<'src> CrateNameRef<'src> {
    pub(crate) fn parse(source: &'src str) -> Result<Self, ()> {
        // This does indeed follow rustc's rules:
        //
        // Crate names are considered to be non-empty Unicode-alphanumeric strings —
        // at least in the context of `--crate-name` and `#![crate_name]`.
        //
        // In the context of extern crates (e.g., in `--extern`), they are considered
        // to be ASCII-only Rust identifiers.
        //
        // However, we don't really need to care about the latter case.
        if !source.is_empty() && source.chars().all(|char| char.is_alphanumeric() || char == '_') {
            Ok(Self::new_unchecked(source))
        } else {
            Err(())
        }
    }
}

impl CrateNameBuf {
    pub(crate) fn adjust_and_parse_file_path(path: &Path) -> Result<Self, ()> {
        path.file_stem().and_then(|name| name.to_str()).ok_or(()).and_then(Self::adjust_and_parse)
    }

    pub(crate) fn adjust_and_parse(source: &str) -> Result<Self, ()> {
        // NB: See the comment over in `CrateNameRef::parse` for why this makes sense.
        if !source.is_empty()
            && source.chars().all(|char| char.is_alphanumeric() || char == '_' || char == '-')
        {
            Ok(Self::new_unchecked(source.replace('-', "_")))
        } else {
            Err(())
        }
    }
}

impl<T: AsRef<str>> CrateName<T> {
    pub(crate) fn as_ref(&self) -> CrateNameRef<'_> {
        CrateName(self.0.as_ref())
    }
}

impl From<CrateNameBuf> for CrateNameCow<'_> {
    fn from(name: CrateNameBuf) -> Self {
        name.map(Cow::Owned)
    }
}

impl<'a> From<CrateNameRef<'a>> for CrateNameCow<'a> {
    fn from(name: CrateNameRef<'a>) -> Self {
        name.map(Cow::Borrowed)
    }
}

impl<T: AsRef<str>> fmt::Display for CrateName<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DocBackend {
    Html,
    Json,
}
