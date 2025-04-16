use crate::{build::EngineKind, context::Context, diagnostic::fmt, utility::paint::Painter};
use anstyle::{AnsiColor, Effects};
use std::{
    borrow::Cow,
    fmt,
    io::{self, Write as _},
    num::NonZero,
    path::Path,
};

#[cfg(test)]
mod test;

pub(crate) enum ExtEdition<'a> {
    EngineDefault,
    LatestStable,
    LatestUnstable,
    Latest,
    Fixed(Edition<'a>),
}

impl<'a> ExtEdition<'a> {
    pub(crate) fn resolve(self, engine: EngineKind, cx: Context<'_>) -> Option<Edition<'a>> {
        match self {
            // FIXME: Return `None` for older engines where editions/epochs don't exist yet!
            Self::EngineDefault => Some(Edition::Rust2015),
            Self::LatestStable => Edition::latest_stable(engine, cx),
            // FIXME: Implement.
            Self::LatestUnstable | Self::Latest => None,
            Self::Fixed(edition) => Some(edition),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Edition<'a> {
    Rust2015,
    Rust2018,
    // FIXME: Genesis: https://github.com/rust-lang/rust/pull/79576
    Rust2021,
    Rust2024,
    // <rust-lang/rust#137606>
    Future,
    // For forward compatibility with future versions of Rust. I don't want to assume that
    // rruxwry gets *so* well maintained that it can keep pace with the development of Rust.
    Unknown(&'a str),
}

impl<'a> Edition<'a> {
    // FIXME: These dates and versions have been manually verified *with rustc*.
    //        It's possible that there are differences to rustdoc. Audit!
    fn latest_stable(engine: EngineKind, cx: Context<'_>) -> Option<Self> {
        // FIXME: Should we warn on failure?
        let version = cx.engine(engine).ok()?;
        match version.channel {
            Channel::Stable => match () {
                () if version.triple >= V!(1, 85, 0) => Some(Self::Rust2024), // branched: 2025-01-03
                () if version.triple >= V!(1, 56, 0) => Some(Self::Rust2021), // branched: 2021-09-03
                () if version.triple >= V!(1, 31, 0) => Some(Self::Rust2018), // branched: 2018-10-19
                // <rust-lang/rust#50080>
                // Before that, a stable edition flag didn't exist.
                () if version.triple >= V!(1, 27, 0) => Some(Self::Rust2015), // branched: 2018-05-04
                () => None,
            },
            Channel::Beta { prerelease: _ } => None, // FIXME: Unimplemented.
            Channel::Nightly | Channel::Dev => match &version.commit {
                Some(commit) => {
                    match () {
                        () if commit.date >= D!(2024, 11, 24) => Some(Self::Rust2024), // base: 1.85.0
                        () if commit.date >= D!(2021, 08, 31) => Some(Self::Rust2021), // base: 1.56.0
                        // <rust-lang/rust#54057>
                        // Note: Back then, unstable editions didn't require you to pass
                        // `-Zunstable-options`. Being on the nightly was sufficient.
                        () if commit.date >= D!(2018, 09, 09) => Some(Self::Rust2018), // base: 1.30.0 (branched: 2018-09-07)
                        // <rust-lang/rust#50080> stabilized `-Zedition` as `--edition`,
                        // turn value `2015` stable and kept `2018` unstable.
                        () if commit.date >= D!(2018, 04, 21) => Some(Self::Rust2015), // base: 1.27.0
                        () => None,
                    }
                }
                _ => {
                    // FIXME: If there isn't commit info we need to go by date.
                    //        it's 50/50 whether it coincides with stable_release or
                    //        not since it could be a nightly from before the stabilization PR
                    //        so check against ONE_PATCH_LESS(stable_release)
                    // FIXME: Figure out if we can figure this out procedually from other data
                    match () {
                        () if version.triple >= V!(1, 86, 0) => Some(Self::Rust2024),
                        () if version.triple >= V!(1, 57, 0) => Some(Self::Rust2021),
                        () if version.triple >= V!(1, 31, 0) => Some(Self::Rust2018),
                        () if version.triple >= V!(1, 28, 0) => Some(Self::Rust2015),
                        () => None,
                    }
                }
            },
        }
    }

    pub(crate) const fn to_str(self) -> &'a str {
        match self {
            Self::Rust2015 => "2015",
            Self::Rust2018 => "2018",
            Self::Rust2021 => "2021",
            Self::Rust2024 => "2024",
            Self::Future => "future",
            Self::Unknown(edition) => edition,
        }
    }
}

// FIXME: Everywhere: Experiment with "inverting" this mapping for maintainability.
//        I.e., have a map from ResultTy (e.g., Edition, Syntax) to a struct of the
//        rough form { stable: Result<Triple, Unsupported>,
//                       beta: Result<(Triple, Result<Pre, Unsupported>), Unsupported>,
//                       nightly: Result<(Date, Result<Triple, Ambiguous>), Unsupported> }.
//        And have a helper function for the performing the actual match

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
pub(crate) struct Crate<'a, E = Edition<'a>> {
    pub(crate) path: Option<&'a Path>,
    pub(crate) name: Option<CrateName<&'a str>>,
    pub(crate) typ: Option<CrateType>,
    pub(crate) edition: Option<E>,
}

#[derive(Clone)] // FIXME
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct Version<S: AsRef<str>> {
    pub(crate) triple: VersionTriple,
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

        let triple = {
            let mut parts = version.split('.');
            let major = parts.next().unwrap().parse().ok()?; // unwrap: `split` never returns an empty iterator
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next()?.parse().ok()?;
            if parts.next().is_some() {
                return None;
            }
            VersionTriple { major, minor, patch }
        };

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
            // FIXME: Reject empty parenthesized tag " ()" (since that's never generated by rust{,do}c).
            Some(source) => source.strip_prefix('(')?.strip_suffix(')')?,
        };

        Some(Self { triple, channel, commit, tag })
    }

    pub(crate) fn into_owned(self) -> Version<String> {
        Version { commit: self.commit.map(Commit::into_owned), tag: self.tag.to_owned(), ..self }
    }
}

impl<S: AsRef<str>> Version<S> {
    pub(crate) fn paint(
        &self,
        identity: Identity,
        p: &mut Painter<impl io::Write>,
    ) -> io::Result<()> {
        {
            let VersionTriple { major, minor, patch } = self.triple;
            let (color, highlight) = match self.channel {
                Channel::Stable | Channel::Beta { .. } => (AnsiColor::BrightWhite, Effects::BOLD),
                Channel::Nightly | Channel::Dev => (AnsiColor::BrightBlack, Effects::new()),
            };
            p.set(color)?;
            write!(p, "{major}.")?;
            p.with(highlight, fmt!("{minor}"))?;
            write!(p, ".{patch} ")?;
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
            write!(p, " [{}]", tag.escape_debug())?;
            p.unset()?;
        }

        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct VersionTriple {
    pub(crate) major: u16,
    pub(crate) minor: u16,
    pub(crate) patch: u16,
}

pub(crate) macro V($major:expr, $minor:expr, $patch:expr) {
    VersionTriple { major: $major, minor: $minor, patch: $patch }
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum Channel {
    Stable,
    // Indeed, `beta` != `beta.0` according to bootstrap.
    Beta { prerelease: Option<u8> },
    Nightly,
    Dev,
}

impl Channel {
    /// Whether unstable features and options are allowed on this channel.
    pub(crate) fn allows_unstable(self) -> bool {
        match self {
            Self::Stable | Self::Beta { .. } => false,
            Self::Nightly | Self::Dev => true,
        }
    }

    fn paint(self, identity: Identity, p: &mut Painter<impl io::Write>) -> io::Result<()> {
        const STABLE: AnsiColor = AnsiColor::White;
        const NIGHTLY: AnsiColor = AnsiColor::Blue;

        let (text, color) = match self {
            Self::Stable => ("stable", STABLE),
            Self::Beta { prerelease: _ } => ("beta", AnsiColor::Yellow),
            Self::Nightly => ("nightly", NIGHTLY),
            Self::Dev => ("dev", AnsiColor::Red),
        };

        // While we could hide "false" identities if they coincide with the "true"
        // one (i.e., Channel::Stable <-> Identity::Stable and Channel::Nightly
        // <-> Identity::Nightly), I don't think it's worth it and it might actually
        // be confusing to users.

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

#[derive(Clone)] // FIXME
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct Commit<S: AsRef<str>> {
    pub(crate) short_sha: S,
    pub(crate) date: Date,
}

impl Commit<&str> {
    fn into_owned(self) -> Commit<String> {
        Commit { short_sha: self.short_sha.to_owned(), ..self }
    }
}

#[derive(Clone)] // FIXME
#[derive(PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct Date {
    pub(crate) year: u16,
    pub(crate) month: NonZero<u8>,
    pub(crate) day: NonZero<u8>,
}

pub(crate) macro D($year:literal, $month:literal, $day:literal) {
    const {
        #[allow(clippy::zero_prefixed_literal)]
        Date { year: $year, month: NonZero::new($month).unwrap(), day: NonZero::new($day).unwrap() }
    }
}

impl Date {
    fn paint(&self, p: &mut Painter<impl io::Write>) -> io::Result<()> {
        write!(p, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}
