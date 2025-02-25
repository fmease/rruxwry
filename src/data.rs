use std::{borrow::Cow, fmt, path::Path};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Edition<'a> {
    Rust2015,
    Rust2018,
    Rust2021,
    Rust2024,
    // For forward compatibility with future versions of Rust.
    // I don't want to assume that rruxwry gets *so* well maintained that
    // it can keep pace with the development of Rust.
    Unknown(&'a str),
}

impl<'a> Edition<'a> {
    pub(crate) const RUSTC_DEFAULT: Self = Self::Rust2015;
    pub(crate) const LATEST_STABLE: Self = Self::Rust2024;
    pub(crate) const BLEEDING_EDGE: Self = Self::LATEST_STABLE;

    pub(crate) fn is_stable(self) -> bool {
        self <= Self::LATEST_STABLE
    }

    pub(crate) const fn to_str(self) -> &'a str {
        match self {
            Self::Rust2015 => "2015",
            Self::Rust2018 => "2018",
            Self::Rust2021 => "2021",
            Self::Rust2024 => "2024",
            Self::Unknown(edition) => edition,
        }
    }
}

// This is just a wrapper around a string. An enum listing all crate types which are
// valid at the time of writing wouldn't be forward compatible with future versions
// of rust{,do}c. I don't want to assume that rruxwry gets *so* well maintained that
// it can keep pace with rust{,do}c.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
// FIXME: Switch to <'a> &'a str once we've thrown out clap.
pub(crate) struct CrateType(pub &'static str);

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

impl<'src> CrateName<&'src str> {
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

impl CrateName<String> {
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
    pub(crate) fn as_ref(&self) -> CrateName<&str> {
        CrateName(self.0.as_ref())
    }
}

impl From<CrateName<String>> for CrateName<Cow<'_, str>> {
    fn from(name: CrateName<String>) -> Self {
        name.map(Cow::Owned)
    }
}

impl<'a> From<CrateName<&'a str>> for CrateName<Cow<'a, str>> {
    fn from(name: CrateName<&'a str>) -> Self {
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

#[derive(Clone, Copy)]
pub(crate) enum Identity {
    True,
    Stable,
    Nightly,
}

#[derive(Clone, Copy)]
pub(crate) struct Crate<'a> {
    pub(crate) path: &'a Path,
    pub(crate) name: Option<CrateName<&'a str>>,
    pub(crate) typ: Option<CrateType>,
    pub(crate) edition: Option<Edition<'a>>,
}
