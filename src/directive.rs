//! The parser of `ui_test`-style `compiletest`, `htmldocck` and `jsondocck` directives.

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
//        --- enum Flavor { Basic, Extended }

use crate::{
    command::{ExternCrate, VerbatimFlagsBuf},
    data::{CrateNameRef, Edition},
    diagnostic::{self, EmittedError, emit},
    utility::{Conjunction, ListingExt},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    iter::Peekable,
    mem,
    ops::{Deref, DerefMut},
    str::CharIndices,
};

#[cfg(test)]
mod tests;

pub(crate) fn parse(source: &str, scope: Scope) -> Directives<'_> {
    let (directives, buffer) = try_parse(source, scope);
    buffer.release();
    directives
}

fn try_parse(source: &str, scope: Scope) -> (Directives<'_>, ErrorBuffer<'_>) {
    let mut buffer = ErrorBuffer::default();
    let mut directives = Directives::default();

    for line in source.lines() {
        let line = line.trim_start();
        let Some(directive) = line.strip_prefix("//@") else { continue };
        match Directive::parse(directive, scope) {
            Ok(directive) => directives.add(directive),
            Err(error) => buffer.add(error),
        }
    }

    (directives, buffer)
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
        if let DirectiveKind::Revisions(revisions) = directive.kind {
            // FIXME: Emit a warning (-@) / error (-@@) for this:
            // We ignore revision predicates on revisions since that's what `compiletest` does, too.
            self.revisions.extend(revisions);
        } else if let Some(revision) = directive.revision {
            self.uninstantiated.entry(revision).or_default().push(directive.kind);
        } else {
            // We immediately adjoin unconditional directives to prevent needlessly
            // instantiating them over and over later in `Self::instantiate`.
            self.adjoin(directive.kind);
        }
    }

    /// Instantiate all directives that are conditional on a revision.
    // FIXME: Return a proper error type (i.e., don't emit immediately).
    pub(crate) fn instantiated(mut self, revision: Option<&str>) -> Result<Self, EmittedError> {
        let available =
            || self.revisions.iter().map(|revision| format!("`{revision}`")).list(Conjunction::And);

        if let Some(revision) = revision {
            if !self.revisions.contains(revision) {
                // FIXME: Emit a different error if `self.revisions.is_empty()`, one that makes no sense
                return Err(emit!(
                    Error("unknown revision `{revision}`")
                        .note("available revisions are: {}", available())
                ));
            }

            if let Some(directives) = mem::take(&mut self.uninstantiated).get(revision) {
                for directive in directives {
                    self.adjoin(directive.clone());
                }
            }
        } else if !self.revisions.is_empty() {
            // FIXME: Return a proper error type, so the caller can suggest `--rev`
            //        without it resulting in an abstraction layer violation.
            return Err(emit!(
                Error("no revision specified").note("available revisions are: {}", available())
            ));
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

#[derive(Default, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct InstantiatedDirectives<'src> {
    pub(crate) dependencies: Vec<ExternCrate<'src>>,
    pub(crate) build_aux_docs: bool,
    pub(crate) edition: Option<Edition>,
    pub(crate) force_host: bool,
    pub(crate) no_prefer_dynamic: bool,
    pub(crate) revisions: BTreeSet<&'src str>,
    pub(crate) verbatim_flags: VerbatimFlagsBuf<'src>,
    pub(crate) htmldocck: Vec<(HtmlDocCkDirectiveKind, Polarity)>,
    pub(crate) jsondocck: Vec<(JsonDocCkDirectiveKind, Polarity)>,
}

impl<'src> InstantiatedDirectives<'src> {
    fn adjoin(&mut self, directive: DirectiveKind<'src>) {
        match directive {
            DirectiveKind::AuxBuild { path } => {
                self.dependencies.push(ExternCrate::Unnamed { path });
            }
            DirectiveKind::AuxCrate { name, path } => {
                self.dependencies.push(ExternCrate::Named { name, path: path.map(Into::into) });
            }
            DirectiveKind::BuildAuxDocs => self.build_aux_docs = true,
            // These flags can indeed conflict with flags generated by us to implement other directives.
            // However, that's just how it is, they are treated verbatim by `compiletest`, so we do the same.
            DirectiveKind::CompileFlags(flags) => self.verbatim_flags.arguments.extend(flags),
            // FIXME: Emit an error or warning if multiple `edition` directives were specified
            //        just like `compiletest` does (what happens if some of the edition directives are conditional?).
            DirectiveKind::Edition(edition) => self.edition = Some(edition),
            DirectiveKind::ForceHost => self.force_host = true,
            DirectiveKind::NoPreferDynamic => self.no_prefer_dynamic = true,
            DirectiveKind::Revisions(_) => unreachable!(), // Already dealt with in `Directives::add`.
            DirectiveKind::RustcEnv { key, value } => {
                self.verbatim_flags.environment.push((key, Some(value)));
            }
            DirectiveKind::UnsetRustcEnv(key) => self.verbatim_flags.environment.push((key, None)),
            DirectiveKind::HtmlDocCk(directive, polarity) => {
                self.htmldocck.push((directive, polarity));
            }
            DirectiveKind::JsonDocCk(directive, polarity) => {
                self.jsondocck.push((directive, polarity));
            }
        }
    }
}

// FIXME: BTreeMap is the wrong data structure. I think this was meant to say "IndexMap"
// (hash map that preserves insertion order). Should we just go for `Vec<(…, …)>`?
type UninstantiatedDirectives<'src> = BTreeMap<&'src str, Vec<DirectiveKind<'src>>>;

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
struct Directive<'src> {
    revision: Option<&'src str>,
    kind: DirectiveKind<'src>,
}

impl<'src> Directive<'src> {
    fn parse(source: &'src str, scope: Scope) -> Result<Self, Error<'src>> {
        DirectiveParser { chars: source.char_indices().peekable(), source, scope }.execute()
    }
}

// FIXME: Can somehow get rid of this? By merging "adjoin" & "parse-single" I guess?
//        This isn't scalable rn
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
enum DirectiveKind<'src> {
    AuxBuild { path: &'src str },
    // FIXME: compiletest doesn't consider the path to be optional (gate this behind `-@@`).
    AuxCrate { name: CrateNameRef<'src>, path: Option<&'src str> },
    BuildAuxDocs,
    CompileFlags(Vec<&'src str>),
    Edition(Edition),
    // FIXME: Is this actually relevant for rruxwry?
    ForceHost,
    // FIXME: Is this actually relevant for rruxwry?
    NoPreferDynamic,
    Revisions(Vec<&'src str>),
    RustcEnv { key: &'src str, value: &'src str },
    UnsetRustcEnv(&'src str),
    HtmlDocCk(HtmlDocCkDirectiveKind, Polarity),
    JsonDocCk(JsonDocCkDirectiveKind, Polarity),
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum HtmlDocCkDirectiveKind {
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
pub(crate) enum JsonDocCkDirectiveKind {
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

struct DirectiveParser<'src> {
    chars: Peekable<CharIndices<'src>>,
    source: &'src str,
    scope: Scope,
}

impl<'src> DirectiveParser<'src> {
    fn execute(mut self) -> Result<Directive<'src>, Error<'src>> {
        self.parse_whitespace();

        let revision = if self.consume(|char| char == '[') {
            let revision = self.take_while(|char| char != ']').unwrap_or_default();
            self.expect(']').map_err(Error::new)?;
            // FIXME: Warn on empty/blank revision ("literally treated as a revision")
            //        Warn on padded revision ("treated literally (not trimmed)")
            //        Warn on quoted revision and commas inside the revision
            // FIXME: Do we want to trim on `-@@`? And hard error on `-@f` (force)?
            Some(revision)
        } else {
            None
        };

        self.parse_whitespace();

        let directive = self
            .take_while(|char| matches!(char, '-' | '!') || char.is_alphabetic())
            .map_err(Error::new)?;

        self.parse_directive_kind(directive).map(|kind| Directive { revision, kind })
    }

    fn parse_directive_kind(
        &mut self,
        source: &'src str,
    ) -> Result<DirectiveKind<'src>, Error<'src>> {
        let context = ErrorContext::Directive(source);

        match self.parse_base_directive(source) {
            Ok(directive) => return Ok(directive),
            Err(ErrorKind::UnknownDirective(_)) => {}
            Err(error) => return Err(Error::new(error).context(context)),
        }
        let htmldocck = match Self::parse_htmldocck_directive(source) {
            Ok(directive) => Some(directive),
            Err(ErrorKind::UnknownDirective(_)) => None,
            Err(error) => return Err(Error::new(error).context(context)),
        };
        let jsondocck = match Self::parse_jsondocck_directive(source) {
            Ok(directive) => Some(directive),
            Err(ErrorKind::UnknownDirective(_)) => None,
            Err(error) => return Err(Error::new(error).context(context)),
        };

        match (self.scope, htmldocck, jsondocck) {
            (Scope::HtmlDocCk, Some(directive), _) | (Scope::JsonDocCk, _, Some(directive)) => {
                return Ok(directive);
            }
            | (Scope::HtmlDocCk | Scope::Base, None, Some(_))
            | (Scope::JsonDocCk | Scope::Base, Some(_), None)
            | (Scope::Base, Some(_), Some(_)) => {
                // FIXME: Add more context to the error.
                return Err(Error::new(ErrorKind::UnavailableDirective(source)));
            }
            _ => {}
        }

        // FIXME: Import/maintain a list of "all directives" (including "parametrized" ones like `only-*``)
        //        currently recognized by compiletest and don't error/warn on them (unless --verbose ig).
        Err(Error::new(ErrorKind::UnknownDirective(source)))
    }

    fn parse_base_directive(
        &mut self,
        source: &'src str,
    ) -> Result<DirectiveKind<'src>, ErrorKind<'src>> {
        Ok(match source {
            "aux-build" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding
                let path = self.take_remaining_line();
                DirectiveKind::AuxBuild { path }
            }
            // `compiletest` doesn't support extern options like `priv`, `noprelude`, `nounused` or `force`
            // at the time of writing. Therefore, we don't need to deal with them here either.
            // Neither does it support optional paths (`//@ aux-crate:name`).
            "aux-crate" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let name = self.take_while(|char| char != '=' && !char.is_whitespace())?;
                let Ok(name) = CrateNameRef::parse(name) else {
                    return Err(ErrorKind::InvalidValue(name));
                };

                let path = self.consume(|char| char == '=').then(|| self.take_remaining_line());
                DirectiveKind::AuxCrate { name, path }
            }
            // FIXME: Is this available outside of rustdoc tests, too? Check compiletest's behavior!
            //        If not, intro a new scope, Scope::Rustdoc, which is separate from HtmlDocCk/JsonDocCk.
            //        Should probably be sth. akin to Scope::Rustdoc(DocCk) then.
            "build-aux-docs" => DirectiveKind::BuildAuxDocs,
            "compile-flags" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding (before)

                // FIXME: Supported quotes arguments (they shouldn't be split in halves).
                //        Use crate `shlex` for this.
                let arguments = self.take_remaining_line().split_whitespace().collect();
                DirectiveKind::CompileFlags(arguments)
            }
            "edition" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding (before)

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let edition = self.take_while(|char| !char.is_whitespace())?;
                // FIXME: Don't actually try to parse the edition!
                let Ok(edition) = edition.parse() else {
                    return Err(ErrorKind::InvalidValue(edition));
                };

                DirectiveKind::Edition(edition)
            }
            "force-host" => DirectiveKind::ForceHost,
            "no-prefer-dynamic" => DirectiveKind::NoPreferDynamic,
            "revisions" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding
                let mut revisions: Vec<_> = self.take_remaining_line().split_whitespace().collect();
                let count = revisions.len();
                revisions.sort_unstable();
                revisions.dedup();
                if count != revisions.len() {
                    // FIXME: Proper more helpful error message.
                    return Err(ErrorKind::DuplicateRevisions);
                }
                DirectiveKind::Revisions(revisions)
            }
            // `compiletest` only supports a single environment variable per directive.
            "rustc-env" => {
                self.parse_separator(Padding::No)?;
                let line = self.take_remaining_line();

                // FIXME: How does `compiletest` handle the edge cases here?
                let Some((key, value)) = line.split_once('=') else {
                    return Err(ErrorKind::InvalidValue(line));
                };
                DirectiveKind::RustcEnv { key, value }
            }
            "unset-rustc-env" => {
                self.parse_separator(Padding::No)?;
                let variable = self.take_remaining_line();
                DirectiveKind::UnsetRustcEnv(variable)
            }
            // FIXME: proc-macro, exec-env, unset-exec-env, run-flags, doc-flags
            source => return Err(ErrorKind::UnknownDirective(source)),
        })
    }

    // FIXME: Actually parse them fully and do sth. with them, otherwise turn this into a array lookup.
    fn parse_htmldocck_directive(
        source: &'src str,
    ) -> Result<DirectiveKind<'src>, ErrorKind<'src>> {
        let (source, polarity) = Self::parse_polarity(source);
        let kind = match source {
            "count" => HtmlDocCkDirectiveKind::Count,
            "files" => HtmlDocCkDirectiveKind::Files,
            "has" => HtmlDocCkDirectiveKind::Has,
            "has-dir" => HtmlDocCkDirectiveKind::HasDir,
            "hasraw" => HtmlDocCkDirectiveKind::HasRaw,
            "matches" => HtmlDocCkDirectiveKind::Matches,
            "matchesraw" => HtmlDocCkDirectiveKind::MatchesRaw,
            "snapshot" => HtmlDocCkDirectiveKind::Snapshot,
            source => return Err(ErrorKind::UnknownDirective(source)),
        };
        Ok(DirectiveKind::HtmlDocCk(kind, polarity))
    }

    // FIXME: Actually parse them fully and do sth. with them, otherwise turn this into a array lookup.
    fn parse_jsondocck_directive(
        source: &'src str,
    ) -> Result<DirectiveKind<'src>, ErrorKind<'src>> {
        let (source, polarity) = Self::parse_polarity(source);
        let kind = match source {
            "count" => JsonDocCkDirectiveKind::Count,
            "has" => JsonDocCkDirectiveKind::Has,
            "is" => JsonDocCkDirectiveKind::Is,
            "ismany" => JsonDocCkDirectiveKind::IsMany,
            "set" => JsonDocCkDirectiveKind::Set,
            source => return Err(ErrorKind::UnknownDirective(source)),
        };
        Ok(DirectiveKind::JsonDocCk(kind, polarity))
    }

    fn parse_polarity(source: &'src str) -> (&'src str, Polarity) {
        match source.strip_prefix('!') {
            Some(source) => (source, Polarity::Negative),
            None => (source, Polarity::Positive),
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().map(|&(_, char)| char)
    }

    fn advance(&mut self) {
        self.chars.next();
    }

    fn consume(&mut self, predicate: impl FnOnce(char) -> bool) -> bool {
        if let Some(char) = self.peek()
            && predicate(char)
        {
            self.advance();
            return true;
        }
        false
    }

    fn expect(&mut self, expected: char) -> Result<(), ErrorKind<'src>> {
        let Some(char) = self.peek() else {
            return Err(ErrorKind::UnexpectedEndOfInput);
        };
        if char != expected {
            return Err(ErrorKind::UnexpectedToken { found: char, expected });
        }
        self.advance();
        Ok(())
    }

    fn advance_while(&mut self, predicate: impl Fn(char) -> bool) {
        while let Some(char) = self.peek() {
            if !predicate(char) {
                break;
            }
            self.advance();
        }
    }

    fn parse_whitespace(&mut self) {
        self.advance_while(|char| char.is_whitespace());
    }

    fn parse_separator(&mut self, padding: Padding) -> Result<(), ErrorKind<'src>> {
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
    ) -> Result<&'src str, ErrorKind<'src>> {
        let mut start = None;
        let mut end = None;

        while let Some(&(index, char)) = self.chars.peek() {
            if !predicate(char) {
                break;
            }
            start.get_or_insert(index);
            end = Some(index + char.len_utf8());
            self.advance();
        }

        match start.zip(end) {
            Some((start, end)) => Ok(&self.source[start..end]),
            None => Err(ErrorKind::UnexpectedEndOfInput),
        }
    }

    fn take_remaining_line(&mut self) -> &'src str {
        // FIXME: Should we instead bail on empty lines?
        self.take_while(|char| char != '\n').unwrap_or_default()
    }
}

#[derive(Default)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
struct ErrorBuffer<'src> {
    errors: Vec<Error<'src>>,
    unknowns: BTreeSet<&'src str>,
    unavailables: BTreeSet<&'src str>,
}

impl<'src> ErrorBuffer<'src> {
    fn add(&mut self, error: Error<'src>) {
        // We coalesce certain kinds of errors where we assume they may occur in large
        // quantities in order to avoid "terminal spamming".
        match error.kind {
            // FIXME: Add rationale
            ErrorKind::UnknownDirective(directive) => {
                self.unknowns.insert(directive);
            }
            // FIXME: Add rationale
            ErrorKind::UnavailableDirective(directive) => {
                self.unavailables.insert(directive);
            }
            _ => self.errors.push(error),
        }
    }

    // FIXME: Shouldn't all these errors be emitted as (non-fatal) errors instead of warnings?
    //        So we can use warnings for something else?
    fn release(self) {
        use std::io::Write;

        // FIXME: Use utility::ListExt::list (once that supports painter/writer)
        let list = |p: &mut diagnostic::Painter, mut elements: BTreeSet<_>| {
            if let Some(element) = elements.pop_first() {
                write!(p, "`{element}`")?;
            }
            for element in elements {
                write!(p, ", `{element}`")?;
            }
            Ok(())
        };

        let plural_s = |elements: &BTreeSet<_>| if elements.len() == 1 { "" } else { "s" };

        if !self.unknowns.is_empty() {
            emit!(Warning(|p| {
                write!(p, "unknown directive{}: ", plural_s(&self.unknowns))?;
                list(p, self.unknowns)
            }));
        }

        if !self.unavailables.is_empty() {
            emit!(Warning(|p| {
                // FIXME: Better error message.
                write!(p, "unavailable directive{}: ", plural_s(&self.unavailables))?;
                list(p, self.unavailables)
            }));
        }

        for error in self.errors {
            emit!(Warning("{error}"));
        }
    }
}

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
struct Error<'src> {
    kind: ErrorKind<'src>,
    context: Option<ErrorContext<'src>>,
}

impl<'src> Error<'src> {
    fn new(kind: ErrorKind<'src>) -> Self {
        Self { kind, context: None }
    }

    fn context(self, context: ErrorContext<'src>) -> Self {
        Self { context: Some(context), ..self }
    }
}

impl fmt::Display for Error<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(context) = self.context {
            write!(f, " {context}")?;
        }
        Ok(())
    }
}

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
enum ErrorKind<'src> {
    UnknownDirective(&'src str),
    // FIXME: add scope?
    UnavailableDirective(&'src str),
    UnexpectedToken { found: char, expected: char },
    UnexpectedEndOfInput,
    InvalidValue(&'src str),
    DuplicateRevisions,
}

impl fmt::Display for ErrorKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // FIXME: This is unreachable. Model this better
            Self::UnknownDirective(unknown) => write!(f, "unknown directive: `{unknown}`"),
            // FIXME: THis is unreachable. Model this better
            Self::UnavailableDirective(directive) => {
                write!(f, "unavailable directive: `{directive}`")
            }
            Self::UnexpectedToken { found, expected } => {
                write!(f, "found `{found}` but expected `{expected}`")
            }
            Self::UnexpectedEndOfInput => write!(f, "unexpected end of input"),
            Self::InvalidValue(value) => write!(f, "invalid value `{value}`"),
            Self::DuplicateRevisions => write!(f, "duplicate revisions"),
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
enum ErrorContext<'src> {
    Directive(&'src str),
}

impl fmt::Display for ErrorContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directive(name) => write!(f, "in directive `{name}`"),
        }
    }
}

// FIXME: Get rid of this if possible.
#[derive(Clone, Copy)]
enum Padding {
    Yes,
    No,
}
