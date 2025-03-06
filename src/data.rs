use crate::{diagnostic::fmt, utility::paint::Painter};
use anstyle::{AnsiColor, Effects};
use std::{
    borrow::Cow,
    fmt,
    io::{self, Write as _},
    num::NonZero,
    path::Path,
};

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
pub(crate) struct CrateName<S: AsRef<str>>(S);

impl<S: AsRef<str>> CrateName<S> {
    pub(crate) const fn new_unchecked(name: S) -> Self {
        Self(name)
    }

    pub(crate) fn parse(source: S) -> Result<Self, ()> {
        // This does indeed follow rustc's rules:
        //
        // Crate names are considered to be non-empty Unicode-alphanumeric strings —
        // at least in the context of `--crate-name` and `#![crate_name]`.
        //
        // In the context of extern crates (e.g., in `--extern`), they are considered
        // to be ASCII-only Rust identifiers.
        //
        // However, we don't really need to care about the latter case.
        if !source.as_ref().is_empty()
            && source.as_ref().chars().all(|char| char.is_alphanumeric() || char == '_')
        {
            Ok(Self(source))
        } else {
            Err(())
        }
    }

    pub(crate) fn map<U: AsRef<str>>(self, mapper: impl FnOnce(S) -> U) -> CrateName<U> {
        CrateName(mapper(self.0))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
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

impl<S: AsRef<str>> CrateName<S> {
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

impl<S: AsRef<str>> fmt::Display for CrateName<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DocBackend {
    Html,
    Json,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Identity {
    #[default]
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

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct Version<S: AsRef<str>> {
    pub(crate) major: u16,
    pub(crate) minor: u16,
    pub(crate) patch: u16,
    pub(crate) channel: Channel,
    pub(crate) commit: Option<Commit<S>>,
    /// Bootstrap calls this the "description" but it's actually a proper part of the version!
    pub(crate) tag: S,
}

impl<'src> Version<&'src str> {
    // FIXME: Maybe reject leading zeroes in numeric components?
    pub(crate) fn parse(source: &'src str) -> Option<Self> {
        let mut words = source.split(' ');

        let (version, channel) = {
            let word = words.next().filter(|word| !word.is_empty())?;
            let mut parts = word.split('-');
            (parts.next().filter(|part| !part.is_empty())?, parts.next())
        };

        let mut parts = version.split('.');
        let major = parts.next().unwrap().parse().ok()?; // unwrap: `split` never returns an empty iterator
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }

        let channel = match channel {
            None => Channel::Stable,
            Some(channel) if let Some(tail) = channel.strip_prefix("beta") => Channel::Beta {
                prerelease: match tail {
                    "" => None,
                    _ => Some(tail.strip_prefix(".")?.parse().ok()?),
                },
            },
            Some("nightly") => Channel::Nightly,
            Some("dev") => Channel::Dev,
            _ => return None,
        };

        // The grammar of the version string is actually ambiguous (that's a bug) since
        // `commit` and `tag` are truly independent inside bootstrap but it's impossible
        // to tell if "… (… …)" contains commit info and no tag or if it contains a tag
        // and no commit info.
        //
        // So we're just always going to interpret the first parenthesized substring as
        // the git info since that's more likely to be correct in practice.
        //
        // We could theoretically interpret " (…)" (i.e., no space inside the parenthesized
        // substring) as the tag since commit info should always contain a space but
        // for now I don't care at all.

        let commit = if let Some(part) = words.next() {
            let short_sha = part.strip_prefix('(')?;
            if short_sha.is_empty() {
                return None;
            }

            let date = words.next()?.strip_suffix(')')?;
            let mut parts = date.split('-');
            let year = parts.next().unwrap().parse().ok()?; // unwrap: `split` never returns an empty iterator
            let month = parts.next()?.parse().ok()?;
            let day = parts.next()?.parse().ok()?;

            Some(Commit { short_sha, date: Date { year, month, day } })
        } else {
            None
        };

        let tag = match words.remainder() {
            None => "",
            Some(source) => source.strip_prefix('(')?.strip_suffix(')')?,
        };

        Some(Self { major, minor, patch, channel, commit, tag })
    }

    pub(crate) fn to_owned(self) -> Version<String> {
        Version { commit: self.commit.map(Commit::to_owned), tag: self.tag.to_owned(), ..self }
    }
}

impl<S: AsRef<str>> Version<S> {
    pub(crate) fn paint(
        &self,
        identity: Identity,
        p: &mut Painter<impl io::Write>,
    ) -> io::Result<()> {
        {
            let (color, highlight) = match self.channel {
                Channel::Stable | Channel::Beta { .. } => (AnsiColor::BrightWhite, Effects::BOLD),
                Channel::Nightly | Channel::Dev => (AnsiColor::BrightBlack, Effects::new()),
            };
            p.set(color)?;
            write!(p, "{}.", self.major)?;
            p.with(highlight, fmt!("{}", self.minor))?;
            write!(p, ".{} ", self.patch)?;
            p.unset()?;
        }

        self.channel.paint(identity, p)?;

        if let Some(commit) = &self.commit {
            write!(p, " ")?;
            p.set(match self.channel {
                Channel::Nightly => AnsiColor::BrightWhite.on_default().bold(),
                _ => AnsiColor::BrightBlack.on_default(),
            })?;
            commit.date.paint(p)?;
            p.unset()?;
            write!(p, " ")?;
            p.with(AnsiColor::BrightBlack, fmt!("{}", commit.short_sha.as_ref()))?;
        }

        let tag = self.tag.as_ref();
        if !tag.is_empty() {
            p.set(AnsiColor::BrightBlack)?;
            write!(p, r" [{}]", tag.escape_debug())?;
            p.unset()?;
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) enum Channel {
    Stable,
    // FIXME: Shouldn't this just be `u8` (None == Some(0))?
    Beta { prerelease: Option<u8> },
    Nightly,
    Dev,
}
impl Channel {
    fn paint(&self, identity: Identity, p: &mut Painter<impl io::Write>) -> io::Result<()> {
        const STABLE: AnsiColor = AnsiColor::White;
        const NIGHTLY: AnsiColor = AnsiColor::Blue;

        let (text, color) = match self {
            Self::Stable => ("stable", STABLE),
            Self::Beta { prerelease: _ } => ("beta", AnsiColor::Yellow),
            Self::Nightly => ("nightly", NIGHTLY),
            Self::Dev => ("dev", AnsiColor::Red),
        };

        if identity != Identity::True {
            // FIXME: If colors are disabled, surround the "overwritten" channel
            //        with tildes `~` instead.
            p.set(Effects::STRIKETHROUGH)?;
        }

        p.with(color.on_default().bold(), fmt!("{text}"))?;
        if let Self::Beta { prerelease: Some(prerelease) } = self {
            write!(p, ".{prerelease}")?;
        }

        if identity != Identity::True {
            p.unset()?; // strikethrough
            write!(p, r#" ""#)?;
            let (text, color) = match identity {
                Identity::Stable => ("stable", STABLE),
                Identity::Nightly => ("nightly", NIGHTLY),
                Identity::True => unreachable!(),
            };
            p.with(color, fmt!("{text}"))?;
            write!(p, r#"""#)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct Commit<S: AsRef<str>> {
    pub(crate) short_sha: S,
    pub(crate) date: Date,
}

impl Commit<&str> {
    fn to_owned(self) -> Commit<String> {
        Commit { short_sha: self.short_sha.to_owned(), ..self }
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct Date {
    pub(crate) year: u16,
    pub(crate) month: NonZero<u8>,
    pub(crate) day: NonZero<u8>,
}

impl Date {
    fn paint(&self, p: &mut Painter<impl io::Write>) -> io::Result<()> {
        write!(p, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[test]
fn version_empty() {
    assert_eq!(Version::parse(""), None);
}

#[test]
fn version_no_explicit_channel() {
    assert_eq!(
        Version::parse("1.2.3"),
        Some(Version {
            major: 1,
            minor: 2,
            patch: 3,
            channel: Channel::Stable,
            commit: None,
            tag: "",
        })
    );
}

#[test]
fn version_insufficent_numeric_components() {
    assert_eq!(Version::parse("1"), None);
    assert_eq!(Version::parse("1.0"), None);
}

#[test]
fn version_too_many_numeric_components() {
    assert_eq!(Version::parse("1.0.0.0"), None);
}

#[test]
fn version_numeric_components_trailing_dot() {
    assert_eq!(Version::parse("1."), None);
    assert_eq!(Version::parse("1.0."), None);
    assert_eq!(Version::parse("1.0.0."), None);
}

#[test]
fn version_channel() {
    assert_eq!(
        Version::parse("1.20.3-beta"),
        Some(Version {
            major: 1,
            minor: 20,
            patch: 3,
            channel: Channel::Beta { prerelease: None },
            commit: None,
            tag: "",
        })
    );
    assert_eq!(
        Version::parse("1.20.3-nightly"),
        Some(Version {
            major: 1,
            minor: 20,
            patch: 3,
            channel: Channel::Nightly,
            commit: None,
            tag: "",
        })
    );
    assert_eq!(
        Version::parse("1.20.3-dev"),
        Some(Version {
            major: 1,
            minor: 20,
            patch: 3,
            channel: Channel::Dev,
            commit: None,
            tag: "",
        })
    );
}

#[test]
fn version_empty_channel() {
    assert_eq!(Version::parse("1.0.0-"), None);
}

#[test]
fn version_beta_channel_prerelease() {
    assert_eq!(
        Version::parse("1.2.3-beta.144"),
        Some(Version {
            major: 1,
            minor: 2,
            patch: 3,
            channel: Channel::Beta { prerelease: Some(144) },
            commit: None,
            tag: "",
        })
    );
}

#[test]
fn version_commit_info() {
    assert_eq!(
        Version::parse("0.0.0-dev (123456789 2000-01-01)"),
        Some(Version {
            major: 0,
            minor: 0,
            patch: 0,
            channel: Channel::Dev,
            commit: Some(Commit {
                short_sha: "123456789",
                date: Date {
                    year: 2000,
                    month: NonZero::new(1).unwrap(),
                    day: NonZero::new(1).unwrap()
                }
            }),
            tag: "",
        })
    );
}

#[test]
fn version_commit_info_no_channel() {
    assert_eq!(
        Version::parse("999.999.999 (000000000 1970-01-01)"),
        Some(Version {
            major: 999,
            minor: 999,
            patch: 999,
            channel: Channel::Stable,
            commit: Some(Commit {
                short_sha: "000000000",
                date: Date {
                    year: 1970,
                    month: NonZero::new(1).unwrap(),
                    day: NonZero::new(1).unwrap()
                }
            }),
            tag: "",
        })
    );
}

#[test]
fn version_tag() {
    assert_eq!(
        Version::parse("0.0.0 (000000000 0000-01-01) (THIS IS A TAG)"),
        Some(Version {
            major: 0,
            minor: 0,
            patch: 0,
            channel: Channel::Stable,
            commit: Some(Commit {
                short_sha: "000000000",
                date: Date {
                    year: 0,
                    month: NonZero::new(1).unwrap(),
                    day: NonZero::new(1).unwrap()
                }
            }),
            tag: "THIS IS A TAG",
        })
    );
}

#[test]
fn version_trailing_whitespace() {
    assert_eq!(Version::parse(" "), None);
    assert_eq!(Version::parse("0.0.0 "), None);
    assert_eq!(Version::parse("0.0.0-dev "), None);
    assert_eq!(Version::parse("0.0.0-dev (000000000 0000-01-01) "), None);
    assert_eq!(Version::parse("0.0.0-dev (000000000 0000-01-01) (TAG) "), None);
}

#[test]
fn version_double_whitespace() {
    assert_eq!(Version::parse("0.0.0-dev  (000000000 0000-01-01)"), None);
    assert_eq!(Version::parse("0.0.0-dev (000000000  0000-01-01)"), None);
}

// Nobody say the tag can't be multiline, right?
// FIXME: Double-check bootstrap.
#[test]
fn version_multiline_tag() {
    assert_eq!(
        Version::parse("0.0.0 (abcdef 0000-01-01) (this\nis\nspanning\nacross\nlines)"),
        Some(Version {
            major: 0,
            minor: 0,
            patch: 0,
            channel: Channel::Stable,
            commit: Some(Commit {
                short_sha: "abcdef",
                date: Date {
                    year: 0,
                    month: NonZero::new(1).unwrap(),
                    day: NonZero::new(1).unwrap()
                }
            }),
            tag: "this\nis\nspanning\nacross\nlines"
        })
    );
}
