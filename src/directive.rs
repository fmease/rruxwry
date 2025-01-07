//! The parser of `ui_test`-style `compiletest`, `htmldocck` and `jsondocck` directives.

// FIXME: Source locations for errors!

// FIXME: Does compiletest permit `//@[pre] check-pass` `//@ revisions: pre`?
// FIXME: We should warn on `//@[undeclared] compile-flags:`.
// FIXME: What does compiletest do on `//@ revisions: dupe dupe`? We should warn.
// FIXME: Warn(-@)/error(-@@) on `//@ revisions: single` (cuz it's useless)

// FIXME: Mirror compiletest regarding `mod …`/`fn …` (sth. like that):
//        Under `-@` ignore all directives below a line starts with `mod …`/`fn …`(…)
//        and issue warnings.
//        Under `-@@` don't ignore them.

// FIXME: Warn/error on "unused"/extraneous arguments (e.g., "//@ build-aux-docs some extra garbage").

// FIXME: ---
//        Be more conservative than compiletest by default. User can use `--force` to
//        downgrade (hard) errors to warnings.
//        Then, `-@@` doesn't mean "stricter" but purely "extended" which includes:
//             * logic predicates inside revision "refs"
//             * inline crates
//             * path-less aux-crate etc.
//        --- enum Flavor { Vanilla, Rruxwry }

use crate::{
    command::{ExternCrate, VerbatimFlagsBuf},
    data::{CrateNameRef, Edition},
    diagnostic::{EmittedError, LineSpan, Location, error, fmt, warn},
    utility::{Conjunction, ListingExt},
};
use std::{
    collections::BTreeSet,
    iter::Peekable,
    mem,
    ops::{Deref, DerefMut, Range},
    path::Path,
    str::CharIndices,
};

#[cfg(test)]
mod tests;

pub(crate) fn gather<'src>(
    source: &'src str,
    path: &Path,
    scope: Scope,
    revision: Option<&str>,
) -> Result<Directives<'src>, EmittedError> {
    let mut errors = ErrorBuffer::default();
    let directives = parse(source, scope, &mut errors);
    errors.release(source, path);
    directives.instantiate(revision).map_err(InstantiationError::emit)
}

fn parse<'src>(
    source: &'src str,
    scope: Scope,
    errors: &mut ErrorBuffer<'src>,
) -> Directives<'src> {
    let mut directives = Directives::default();

    for (index, line) in source.lines().enumerate() {
        let Some(directive) = line.trim_start().strip_prefix("//@") else { continue };
        let offset = (
            index.try_into().unwrap(),
            line.substr_range(directive).unwrap().start.try_into().unwrap(),
        );

        match Parser::new(directive, scope, offset).parse_directive() {
            Ok(directive) => directives.add(directive),
            Err(error) => errors.insert(error),
        }
    }

    errors.extend(
        directives
            .uninstantiated
            .iter()
            .filter(|&((revision, _), _)| !directives.revisions.contains(revision))
            .map(|&(revision, _)| Error::UndeclaredRevision {
                revision,
                available: directives.revisions.clone(),
            }),
    );

    directives
}

#[derive(Clone, Copy)]
pub(crate) enum Scope {
    Base,
    HtmlDocCk,
    JsonDocCk,
}

// FIXME: If possible get rid of the instantiated vs. uninstantiated separation.
//        Users can no longer specify multiple revisions at once, so we don't
//        need to care about "optimizing" unconditional directives.
#[derive(Default)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct Directives<'src> {
    instantiated: InstantiatedDirectives<'src>,
    uninstantiated: UninstantiatedDirectives<'src>,
}

impl<'src> Directives<'src> {
    fn add(&mut self, directive: Directive<'src>) {
        if let BareDirective::Revisions(revisions) = directive.bare {
            // FIXME: Emit a warning (-@) / error (-@@) for this:
            // We ignore revision predicates on revisions since that's what `compiletest` does, too.
            self.revisions.extend(revisions);
        } else if let Some(revision) = directive.revision {
            self.uninstantiated.push((revision, directive.bare));
        } else {
            // We immediately adjoin unconditional directives to prevent needlessly
            // instantiating them over and over later in `Self::instantiate`.
            self.adjoin(directive.bare);
        }
    }

    /// Instantiate all directives that are conditional on a revision.
    fn instantiate(
        mut self,
        active_revision: Option<&str>,
    ) -> Result<Self, InstantiationError<'src, '_>> {
        let revisions = mem::take(&mut self.revisions);
        let directives = mem::take(&mut self.uninstantiated);

        if let Some(active_revision) = active_revision {
            if !revisions.contains(active_revision) {
                return Err(InstantiationError::UndeclaredActiveRevision {
                    revision: active_revision,
                    available: revisions,
                });
            }

            for ((revision, _), directive) in directives {
                if revision == active_revision {
                    self.adjoin(directive);
                }
            }
        } else if !revisions.is_empty() {
            return Err(InstantiationError::MissingActiveRevision { available: revisions });
        }

        Ok(self)
    }
}

impl<'src> Deref for Directives<'src> {
    type Target = InstantiatedDirectives<'src>;

    fn deref(&self) -> &Self::Target {
        &self.instantiated
    }
}

impl DerefMut for Directives<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.instantiated
    }
}

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
enum InstantiationError<'src, 'rev> {
    UndeclaredActiveRevision { revision: &'rev str, available: BTreeSet<&'src str> },
    MissingActiveRevision { available: BTreeSet<&'src str> },
}

impl InstantiationError<'_, '_> {
    fn emit(self) -> EmittedError {
        let list = |available: BTreeSet<_>| {
            available.into_iter().map(|revision| format!("`{revision}`")).list(Conjunction::And)
        };

        // FIXME: Improve the phrasing of these diagnostics!
        match self {
            // FIXME: Emit a better error if `available.is_empty()`
            Self::UndeclaredActiveRevision { revision, available } => {
                error(fmt!("undeclared revision `{revision}`"))
                    .note(fmt!("available revisions are: {}", list(available)))
                    .finish()
            }
            // FIXME: Suggest `--rev` without it resulting in an abstraction layer violation.
            Self::MissingActiveRevision { available } => error(fmt!("no revision specified"))
                .note(fmt!("available revisions are: {}", list(available)))
                .finish(),
        }
    }
}

#[derive(Default, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct InstantiatedDirectives<'src> {
    pub(crate) dependencies: Vec<ExternCrate<'src>>,
    pub(crate) build_aux_docs: bool,
    pub(crate) edition: Option<Edition>,
    pub(crate) verbatim_flags: VerbatimFlagsBuf<'src>,
    pub(crate) htmldocck: Vec<(HtmlDocCkDirective, Polarity)>,
    pub(crate) jsondocck: Vec<(JsonDocCkDirective, Polarity)>,
    revisions: BTreeSet<&'src str>,
}

impl<'src> InstantiatedDirectives<'src> {
    fn adjoin(&mut self, directive: BareDirective<'src>) {
        match directive {
            BareDirective::AuxBuild { path } => {
                self.dependencies.push(ExternCrate::Unnamed { path });
            }
            BareDirective::AuxCrate { name, path } => {
                self.dependencies.push(ExternCrate::Named { name, path: path.map(Into::into) });
            }
            BareDirective::BuildAuxDocs => self.build_aux_docs = true,
            // These flags can indeed conflict with flags generated by us to implement other directives.
            // However, that's just how it is, they are treated verbatim by `compiletest`, so we do the same.
            BareDirective::CompileFlags(flags) => self.verbatim_flags.arguments.extend(flags),
            // FIXME: Emit an error or warning if multiple `edition` directives were specified
            //        just like `compiletest` does.
            //        When encountering unconditional+conditional, emit a warning(-@)/error(-@@)
            BareDirective::Edition(edition) => self.edition = Some(edition),
            BareDirective::Revisions(_) => unreachable!(), // Already dealt with in `Directives::add`.
            BareDirective::RustcEnv { key, value } => {
                self.verbatim_flags.environment.push((key, Some(value)));
            }
            BareDirective::UnsetRustcEnv(key) => self.verbatim_flags.environment.push((key, None)),
            BareDirective::HtmlDocCk(directive, polarity) => {
                self.htmldocck.push((directive, polarity));
            }
            BareDirective::JsonDocCk(directive, polarity) => {
                self.jsondocck.push((directive, polarity));
            }
        }
    }
}

type UninstantiatedDirectives<'src> = Vec<((&'src str, LineSpan), BareDirective<'src>)>;

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
struct Directive<'src> {
    revision: Option<(&'src str, LineSpan)>,
    bare: BareDirective<'src>,
}

// FIXME: Can somehow get rid of this? By merging "adjoin" & "parse-single" I guess?
//        This isn't scalable rn
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
enum BareDirective<'src> {
    AuxBuild { path: &'src str },
    // FIXME: compiletest doesn't consider the path to be optional (gate this behind `-@@`).
    AuxCrate { name: CrateNameRef<'src>, path: Option<&'src str> },
    BuildAuxDocs,
    CompileFlags(Vec<&'src str>),
    Edition(Edition),
    Revisions(Vec<&'src str>),
    RustcEnv { key: &'src str, value: &'src str },
    UnsetRustcEnv(&'src str),
    HtmlDocCk(HtmlDocCkDirective, Polarity),
    JsonDocCk(JsonDocCkDirective, Polarity),
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum HtmlDocCkDirective {
    Count,
    Files,
    Has,
    HasDir,
    HasRaw,
    Matches,
    MatchesRaw,
    Snapshot,
}

// FIXME: Populate payloads
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum JsonDocCkDirective {
    Count,
    Has,
    Is,
    IsMany,
    Set,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum Polarity {
    Negative,
    Positive,
}

struct Parser<'src> {
    chars: Peekable<CharIndices<'src>>,
    source: &'src str,
    scope: Scope,
    offset: (u32, u32),
}

impl<'src> Parser<'src> {
    fn new(source: &'src str, scope: Scope, offset: (u32, u32)) -> Self {
        Self { chars: source.char_indices().peekable(), source, scope, offset }
    }

    fn parse_directive(mut self) -> Result<Directive<'src>, Error<'src>> {
        self.parse_whitespace();

        let revision = if self.consume(|char| char == '[') {
            // FIXME: Warn on empty/blank revision ("literally treated as a revision")
            //        Warn on padded revision ("treated literally (not trimmed)")
            //        Warn on quoted revision and commas inside the revision
            // FIXME: Do we want to trim on `-@@`? And hard error on `-@f` (force)
            // FIXME: 0..0 is incorrect for empty revision, should be cur_idx..cur_idx
            let range = self.take_while(|char| char != ']').unwrap_or(0..0);
            self.expect(']')?;

            Some((self.source(range.clone()), self.span(range)))
        } else {
            None
        };

        self.parse_whitespace();

        let directive = self
            .take_while(|char| matches!(char, '-' | '!') || char.is_alphabetic())
            .map(|range| self.source(range))?;

        self.parse_bare_directive(directive)
            .map(|directive| Directive { revision, bare: directive })
    }

    fn parse_bare_directive(
        &mut self,
        source: &'src str,
    ) -> Result<BareDirective<'src>, Error<'src>> {
        match self.parse_base_directive(source) {
            Ok(directive) => return Ok(directive),
            Err(Error::UnknownDirective(_)) => {}
            result @ Err(_) => return result,
        }
        let htmldocck = match Self::parse_htmldocck_directive(source) {
            Ok(directive) => Some(directive),
            Err(Error::UnknownDirective(_)) => None,
            result @ Err(_) => return result,
        };
        let jsondocck = match Self::parse_jsondocck_directive(source) {
            Ok(directive) => Some(directive),
            Err(Error::UnknownDirective(_)) => None,
            result @ Err(_) => return result,
        };

        match (self.scope, htmldocck, jsondocck) {
            (Scope::HtmlDocCk, Some(directive), _) | (Scope::JsonDocCk, _, Some(directive)) => {
                return Ok(directive);
            }
            | (Scope::HtmlDocCk | Scope::Base, None, Some(_))
            | (Scope::JsonDocCk | Scope::Base, Some(_), None)
            | (Scope::Base, Some(_), Some(_)) => {
                // FIXME: Add more context to the error.
                return Err(Error::UnavailableDirective(source));
            }
            _ => {}
        }

        // FIXME: Import/maintain a list of "all directives" (including "parametrized" ones like `only-*``)
        //        currently recognized by compiletest and don't error/warn on them (unless --verbose ig).
        Err(Error::UnknownDirective(source))
    }

    fn parse_base_directive(
        &mut self,
        source: &'src str,
    ) -> Result<BareDirective<'src>, Error<'src>> {
        Ok(match source {
            "aux-build" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding
                let path = self.take_remaining_line();
                BareDirective::AuxBuild { path }
            }
            // `compiletest` doesn't support extern options like `priv`, `noprelude`, `nounused` or `force`
            // at the time of writing. Therefore, we don't need to deal with them here either.
            // Neither does it support optional paths (`//@ aux-crate:name`).
            "aux-crate" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let name = self
                    .take_while(|char| char != '=' && !char.is_whitespace())
                    .map(|range| self.source(range))?;
                let Ok(name) = CrateNameRef::parse(name) else {
                    return Err(Error::InvalidValue(name));
                };

                let path = self.consume(|char| char == '=').then(|| self.take_remaining_line());
                BareDirective::AuxCrate { name, path }
            }
            // FIXME: Is this available outside of rustdoc tests, too? Check compiletest's behavior!
            //        If not, intro a new scope, Scope::Rustdoc, which is separate from HtmlDocCk/JsonDocCk.
            //        Should probably be sth. akin to Scope::Rustdoc(DocCk) then.
            "build-aux-docs" => BareDirective::BuildAuxDocs,
            "compile-flags" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding (before)

                // FIXME: Supported quotes arguments (they shouldn't be split in halves).
                //        Use crate `shlex` for this.
                let arguments = self.take_remaining_line().split_whitespace().collect();
                BareDirective::CompileFlags(arguments)
            }
            "edition" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding (before)

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let edition = self
                    .take_while(|char| !char.is_whitespace())
                    .map(|range| self.source(range))?;
                // FIXME: Don't actually try to parse the edition!
                let Ok(edition) = edition.parse() else {
                    return Err(Error::InvalidValue(edition));
                };

                BareDirective::Edition(edition)
            }
            // FIXME: Warn/error if we're inside of an auxiliary file.
            //        ->Warn: "directive gets ignored // revisions are inherited in aux"
            //        ->Error: "directive not permitted // revisions are inherited in aux"
            //        For that, introduce a new parameter: PermitRevisionDeclarations::{Yes, No}.
            "revisions" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding
                let mut revisions: Vec<_> = self.take_remaining_line().split_whitespace().collect();
                let count = revisions.len();
                revisions.sort_unstable();
                revisions.dedup();
                if count != revisions.len() {
                    // FIXME: Proper more helpful error message.
                    return Err(Error::DuplicateRevisions);
                }
                BareDirective::Revisions(revisions)
            }
            // `compiletest` only supports a single environment variable per directive.
            "rustc-env" => {
                self.parse_separator(Padding::No)?;
                let line = self.take_remaining_line();

                // FIXME: How does `compiletest` handle the edge cases here?
                let Some((key, value)) = line.split_once('=') else {
                    return Err(Error::InvalidValue(line));
                };
                BareDirective::RustcEnv { key, value }
            }
            "unset-rustc-env" => {
                self.parse_separator(Padding::No)?;
                let variable = self.take_remaining_line();
                BareDirective::UnsetRustcEnv(variable)
            }
            // FIXME: Actually support some of these flags. In order of importance:
            //        `doc-flags`, `run-flags`, `exec-env`, `unset-exec-env`,
            //        `proc-macro`, `aux-bin`,
            //        `no-prefer-dynamic` (once our auxes are actually dylibs),
            //        `unique-doc-out-dir` (I think),
            //        `incremental`,
            //        `no-auto-check-cfg` (once we actually automatically check-cfg)
            | "add-core-stubs"
            | "assembly-output"
            | "aux-bin"
            | "aux-codegen-backend"
            | "build-fail"
            | "build-pass"
            | "check-fail"
            | "check-pass"
            | "check-run-results"
            | "check-stdout"
            | "check-test-line-numbers-match"
            | "doc-flags"
            | "dont-check-compiler-stderr"
            | "dont-check-compiler-stdout"
            | "dont-check-failure-status"
            | "error-pattern"
            | "exact-llvm-major-version"
            | "exec-env"
            | "failure-status"
            | "filecheck-flags"
            | "forbid-output"
            | "force-host"
            | "incremental"
            | "known-bug"
            | "llvm-cov-flags"
            | "max-llvm-major-version"
            | "min-cdb-version"
            | "min-gdb-version"
            | "min-lldb-version"
            | "min-llvm-version"
            | "min-system-llvm-version"
            | "no-auto-check-cfg"
            | "no-prefer-dynamic"
            | "normalize-stderr-32bit"
            | "normalize-stderr-64bit"
            | "normalize-stderr-test"
            | "normalize-stdout-test"
            | "pp-exact"
            | "pretty-compare-only"
            | "pretty-mode"
            | "proc-macro"
            | "reference"
            | "regex-error-pattern"
            | "remap-src-base"
            | "run-fail"
            | "run-flags"
            | "run-pass"
            | "run-rustfix"
            | "rustfix-only-machine-applicable"
            | "should-fail"
            | "should-ice"
            | "stderr-per-bitwidth"
            | "test-mir-pass"
            | "unique-doc-out-dir"
            | "unset-exec-env"
            | "unused-revision-names" => {
                return Err(Error::UnsupportedDirective(source));
            }
            _ if source.starts_with("ignore-") => return Err(Error::UnsupportedDirective(source)),
            _ if source.starts_with("needs-") => return Err(Error::UnsupportedDirective(source)),
            _ if source.starts_with("only-") => return Err(Error::UnsupportedDirective(source)),
            _ => return Err(Error::UnknownDirective(source)),
        })
    }

    // FIXME: Actually parse them fully and do sth. with them, otherwise turn this into a array lookup.
    fn parse_htmldocck_directive(source: &'src str) -> Result<BareDirective<'src>, Error<'src>> {
        let (source, polarity) = Self::parse_polarity(source);
        let directive = match source {
            "count" => HtmlDocCkDirective::Count,
            "files" => HtmlDocCkDirective::Files,
            "has" => HtmlDocCkDirective::Has,
            "has-dir" => HtmlDocCkDirective::HasDir,
            "hasraw" => HtmlDocCkDirective::HasRaw,
            "matches" => HtmlDocCkDirective::Matches,
            "matchesraw" => HtmlDocCkDirective::MatchesRaw,
            "snapshot" => HtmlDocCkDirective::Snapshot,
            source => return Err(Error::UnknownDirective(source)),
        };
        Ok(BareDirective::HtmlDocCk(directive, polarity))
    }

    // FIXME: Actually parse them fully and do sth. with them, otherwise turn this into a array lookup.
    fn parse_jsondocck_directive(source: &'src str) -> Result<BareDirective<'src>, Error<'src>> {
        let (source, polarity) = Self::parse_polarity(source);
        let directive = match source {
            "count" => JsonDocCkDirective::Count,
            "has" => JsonDocCkDirective::Has,
            "is" => JsonDocCkDirective::Is,
            "ismany" => JsonDocCkDirective::IsMany,
            "set" => JsonDocCkDirective::Set,
            source => return Err(Error::UnknownDirective(source)),
        };
        Ok(BareDirective::JsonDocCk(directive, polarity))
    }

    fn parse_polarity(source: &'src str) -> (&'src str, Polarity) {
        match source.strip_prefix('!') {
            Some(source) => (source, Polarity::Negative),
            None => (source, Polarity::Positive),
        }
    }

    fn peek(&mut self) -> Option<(usize, char)> {
        self.chars.peek().copied()
    }

    fn advance(&mut self) {
        self.chars.next();
    }

    fn consume(&mut self, predicate: impl FnOnce(char) -> bool) -> bool {
        if let Some((_, char)) = self.peek()
            && predicate(char)
        {
            self.advance();
            return true;
        }
        false
    }

    fn expect(&mut self, expected: char) -> Result<(), Error<'src>> {
        let Some((_, char)) = self.peek() else {
            return Err(Error::UnexpectedEndOfInput);
        };
        if char != expected {
            return Err(Error::UnexpectedToken { found: char, expected });
        }
        self.advance();
        Ok(())
    }

    fn advance_while(&mut self, predicate: impl Fn(char) -> bool) {
        while let Some((_, char)) = self.peek() {
            if !predicate(char) {
                break;
            }
            self.advance();
        }
    }

    fn parse_whitespace(&mut self) {
        self.advance_while(char::is_whitespace);
    }

    fn parse_separator(&mut self, padding: Padding) -> Result<(), Error<'src>> {
        if let Padding::Yes = padding {
            self.parse_whitespace();
        }
        // FIXME: compiletest doesn't require ":" but silently ignores the whole directive
        //        if it's absent. On --force we shouldn't expect but consume and notify
        //        the caller that the directive should be discarded
        self.expect(':')?;
        if let Padding::Yes = padding {
            self.parse_whitespace();
        }
        Ok(())
    }

    fn take_while(
        &mut self,
        predicate: impl Fn(char) -> bool,
    ) -> Result<Range<usize>, Error<'src>> {
        let mut start = None;
        let mut end = None;

        while let Some((index, char)) = self.peek() {
            if !predicate(char) {
                break;
            }
            start.get_or_insert(index);
            end = Some(index + char.len_utf8());
            self.advance();
        }

        match start.zip(end) {
            Some((start, end)) => Ok(start..end),
            None => Err(Error::UnexpectedEndOfInput),
        }
    }

    fn take_remaining_line(&mut self) -> &'src str {
        // FIXME: Should we instead bail on empty lines?
        self.take_while(|char| char != '\n').map(|range| self.source(range)).unwrap_or_default()
    }

    fn source(&self, range: Range<usize>) -> &'src str {
        &self.source[range]
    }

    fn span(&self, range: Range<usize>) -> LineSpan {
        LineSpan {
            line: self.offset.0,
            start: u32::try_from(range.start).unwrap() + self.offset.1,
            end: u32::try_from(range.end).unwrap() + self.offset.1,
        }
    }
}

#[derive(Default)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
struct ErrorBuffer<'src> {
    errors: Vec<Error<'src>>,
    unavailable: BTreeSet<&'src str>,
    unsupported: BTreeSet<&'src str>,
    unknown: BTreeSet<&'src str>,
}

impl<'src> ErrorBuffer<'src> {
    fn insert(&mut self, error: Error<'src>) {
        // We coalesce certain kinds of errors where we assume they may occur in large
        // quantities in order to avoid "terminal spamming".
        match error {
            // FIXME: Add rationale
            Error::UnavailableDirective(directive) => {
                self.unavailable.insert(directive);
            }
            Error::UnsupportedDirective(directive) => {
                self.unsupported.insert(directive);
            }
            // FIXME: Add rationale
            Error::UnknownDirective(directive) => {
                self.unknown.insert(directive);
            }
            _ => self.errors.push(error),
        }
    }

    fn extend(&mut self, errors: impl IntoIterator<Item = Error<'src>>) {
        errors.into_iter().for_each(|error| self.insert(error));
    }

    // FIXME: Shouldn't all these errors be emitted as (non-fatal) errors instead of warnings?
    fn release(self, source: &str, path: &Path) {
        use std::io::Write;

        let emit_grouped = |name: &str, mut errors: BTreeSet<_>| {
            if !errors.is_empty() {
                let s = if errors.len() == 1 { "" } else { "s" };

                // FIXME: Better error message.
                // FIXME: Use utility::ListExt::list (once that supports painter/writer)
                warn(|p| {
                    write!(p, "{name} directive{s}: ")?;
                    if let Some(error) = errors.pop_first() {
                        write!(p, "`{error}`")?;
                    }
                    for error in errors {
                        write!(p, ", `{error}`")?;
                    }
                    Ok(())
                })
                .finish();
            }
        };

        emit_grouped("unavailable", self.unavailable);
        emit_grouped("unsupported", self.unsupported);
        emit_grouped("unknown", self.unknown);

        self.errors.into_iter().for_each(|error| error.emit(source, path));
    }
}

impl Error<'_> {
    fn emit(self, source: &str, path: &Path) {
        // FIXME: Improve the phrasing of these diagnostics!
        match self {
            // FIXME: This is awkward, model your errors better.
            | Self::UnavailableDirective(_)
            | Self::UnsupportedDirective(_)
            | Self::UnknownDirective(_) => {
                // Handled in `ErrorBuffer::release`.
                unreachable!()
            }
            Self::UnexpectedToken { found, expected } => {
                error(fmt!("found `{found}` but expected `{expected}`"))
            }
            Self::UnexpectedEndOfInput => error(fmt!("unexpected end of input")),
            Self::InvalidValue(value) => error(fmt!("invalid value `{value}`")),
            Self::DuplicateRevisions => error(fmt!("duplicate revisions")),
            // FIXME: Incorporate the location!
            Self::UndeclaredRevision { revision: (revision, span), available } => {
                // FIXME: Dedupe w/ InstErr:
                let list = |available: BTreeSet<_>| {
                    available
                        .into_iter()
                        .map(|revision| format!("`{revision}`"))
                        .list(Conjunction::And)
                };

                let error = error(fmt!("undeclared revision `{revision}`")).location(Location {
                    source,
                    path,
                    span,
                });

                if available.is_empty() {
                    error.help(fmt!("consider declaring a revision with the `revisions` directive"))
                } else {
                    // FIXME: Also add the help here to extend the revisisons directive
                    error.note(fmt!("available revisions are: {}", list(available)))
                }
            }
        }
        .finish();
    }
}

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
enum Error<'src> {
    UnavailableDirective(&'src str),
    UnsupportedDirective(&'src str),
    UnknownDirective(&'src str),
    UnexpectedToken { found: char, expected: char },
    UnexpectedEndOfInput,
    InvalidValue(&'src str),
    DuplicateRevisions,
    UndeclaredRevision { revision: (&'src str, LineSpan), available: BTreeSet<&'src str> },
}

// FIXME: Get rid of this if possible.
#[derive(Clone, Copy)]
enum Padding {
    Yes,
    No,
}
