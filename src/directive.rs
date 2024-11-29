//! The parser of `ui_test`-style `compiletest`, `htmldocck` and `jsondocck` directives.

use crate::{
    command::{ExternCrate, VerbatimFlagsBuf},
    data::{CrateNameRef, Edition},
    diagnostic::warning,
    parser,
    utility::default,
};
use joinery::JoinableIterator;
use ra_ap_rustc_lexer::TokenKind;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    fmt,
    iter::Peekable,
    ops::{Deref, DerefMut},
    str::CharIndices,
};

#[derive(Default)]
pub(crate) struct Directives<'src> {
    instantiated: InstantiatedDirectives<'src>,
    uninstantiated: UninstantiatedDirectives<'src>,
}

impl<'src> Directives<'src> {
    pub(crate) fn parse(source: &'src str) -> Self {
        DirectivesParser::new(source).execute()
    }

    fn add(&mut self, directive: Directive<'src>) {
        if let DirectiveKind::Revisions(revisions) = directive.kind {
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
    pub(crate) fn into_instantiated(mut self, revs: &FxHashSet<&str>) -> Self {
        let uninstantiated = std::mem::take(&mut self.uninstantiated);
        Self::instantiate(&mut self, &uninstantiated, revs);
        self
    }

    /// Instantiate all directives that are conditional on a revision.
    #[allow(dead_code)] // FIXME: use this when impl'ing `--all-revs`
    pub(crate) fn instantiated(&self, revs: &FxHashSet<&str>) -> Self {
        let mut instantiated =
            Self { instantiated: self.instantiated.clone(), uninstantiated: default() };
        Self::instantiate(&mut instantiated, &self.uninstantiated, revs);
        instantiated
    }

    fn instantiate(
        instantiated: &mut InstantiatedDirectives<'src>,
        uninstantiated: &UninstantiatedDirectives<'src>,
        revisions: &FxHashSet<&str>,
    ) {
        // In the most common case, the user doesn't enable any revisions. Therefore we
        // iterate over the `revisions` instead of the `uninstantiated` directives and
        // can avoid performing unnecessary work.
        for revision in revisions {
            if let Some(directives) = uninstantiated.get(revision) {
                for directive in directives {
                    instantiated.adjoin(directive.clone());
                }
            }
        }
    }
}

impl<'src> Deref for Directives<'src> {
    type Target = InstantiatedDirectives<'src>;

    fn deref(&self) -> &Self::Target {
        &self.instantiated
    }
}

impl<'src> DerefMut for Directives<'src> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.instantiated
    }
}

struct DirectivesParser<'src> {
    parser: parser::SourceFileParser<'src>,
    directives: Directives<'src>,
}

impl<'src> DirectivesParser<'src> {
    fn new(source: &'src str) -> Self {
        Self { parser: parser::SourceFileParser::new(source), directives: default() }
    }

    // FIXME: Parse htmldocck/jsondocck queries
    fn execute(mut self) -> Directives<'src> {
        let mut report = Report::default();

        while let Some(token) = self.parser.peek() {
            if let TokenKind::LineComment { doc_style: None } = token.kind
                && let comment = self.parser.source()
                && let Some(directive) = comment.strip_prefix("//@")
            {
                match DirectiveParser::new(directive).execute() {
                    Ok(directive) => self.directives.add(directive),
                    // Emit a single error containing all unknown directives to avoid terminal spam.
                    Err(Error { kind: ErrorKind::UnknownDirective(directive), .. }) => {
                        report.unknowns.push(directive)
                    }
                    Err(error) => report.errors.push(error),
                };
            }

            self.parser.advance();
        }

        report.publish();
        self.directives
    }
}

#[derive(Default, Clone)]
pub(crate) struct InstantiatedDirectives<'src> {
    pub(crate) dependencies: Vec<ExternCrate<'src>>,
    pub(crate) build_aux_docs: bool,
    pub(crate) edition: Option<Edition>,
    pub(crate) force_host: bool,
    pub(crate) no_prefer_dynamic: bool,
    pub(crate) revisions: FxHashSet<&'src str>,
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
                self.dependencies.push(ExternCrate::Named { name, path: path.map(Into::into) })
            }
            DirectiveKind::BuildAuxDocs => self.build_aux_docs = true,
            // These flags can indeed conflict with flags generated by us to implement other directives.
            // However, that's just how it is, they are treated verbatim by `compiletest`, so we do the same.
            DirectiveKind::CompileFlags(flags) => self.verbatim_flags.arguments.extend(flags),
            // FIXME: Emit an error or warning if multiple `edition` directives were specified
            //        just like `compiletest` does.
            DirectiveKind::Edition(edition) => self.edition = Some(edition),
            DirectiveKind::ForceHost => self.force_host = true,
            DirectiveKind::NoPreferDynamic => self.no_prefer_dynamic = true,
            DirectiveKind::Revisions(_) => unreachable!(), // Already dealt with in `Self::add`.
            DirectiveKind::RustcEnv { key, value } => {
                self.verbatim_flags.environment.push((key, Some(value)))
            }
            DirectiveKind::UnsetRustcEnv(key) => self.verbatim_flags.environment.push((key, None)),
            DirectiveKind::HtmlDocCk(directive, polarity) => {
                self.htmldocck.push((directive, polarity))
            }
            DirectiveKind::JsonDocCk(directive, polarity) => {
                self.jsondocck.push((directive, polarity))
            }
        }
    }
}

type UninstantiatedDirectives<'src> = FxHashMap<&'src str, Vec<DirectiveKind<'src>>>;

struct Directive<'src> {
    revision: Option<&'src str>,
    kind: DirectiveKind<'src>,
}

#[derive(Clone)]
enum DirectiveKind<'src> {
    AuxBuild { path: &'src str },
    // FIXME: Double-check that the path is indeed optional.
    AuxCrate { name: CrateNameRef<'src>, path: Option<&'src str> },
    // FIXME: This is relevant for rruxwry, right?
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

#[derive(Clone)]
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
#[derive(Clone)]
pub(crate) enum JsonDocCkDirectiveKind {
    #[allow(dead_code)] // FIXME
    Count,
    #[allow(dead_code)] // FIXME
    Has,
    Is,
    IsMany,
    Set,
}

#[derive(Clone, Copy)]
pub(crate) enum Polarity {
    Negative,
    Positive,
}

struct DirectiveParser<'src> {
    chars: Peekable<CharIndices<'src>>,
    source: &'src str,
}

impl<'src> DirectiveParser<'src> {
    fn new(source: &'src str) -> Self {
        Self { chars: source.char_indices().peekable(), source }
    }

    fn execute(mut self) -> Result<Directive<'src>, Error<'src>> {
        self.parse_whitespace();

        // FIXME: Non-standard: Support multiple revisions inside `[`,`]`, e.g. `[a,b]`
        let revision = if self.consume(|char| char == '[') {
            // FIXME: How does `compiletest` deal with empty revision conditions (`//@[] ...`)?
            let revision = self.take_while(|char| char != ']');
            self.expect(']')?;
            Some(revision)
        } else {
            None
        };

        self.parse_whitespace();

        let directive =
            self.take_while(|char| matches!(char, '-' | '!') || char.is_ascii_alphabetic());
        let context = ErrorContext::Directive(directive);
        let kind = match directive {
            "aux-build" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.context(context))?; // FIXME: audit AllowPadding
                let path = self.take_remaining_line();
                DirectiveKind::AuxBuild { path }
            }
            // `compiletest` doesn't support extern options like `priv`, `noprelude`, `nounused` or `force`
            // at the time of writing. Therefore, we don't need to deal with them here either.
            // Neither does it support optional paths (`//@ aux-crate:name`).
            "aux-crate" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.context(context))?; // FIXME: audit AllowPadding

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let name = self.take_while(|char| char != '=' && !char.is_ascii_whitespace());
                let Ok(name) = CrateNameRef::parse(name) else {
                    return Err(Error::new(ErrorKind::InvalidValue(name)).context(context));
                };

                let path = self.consume(|char| char == '=').then(|| self.take_remaining_line());
                DirectiveKind::AuxCrate { name, path }
            }
            "build-aux-docs" => DirectiveKind::BuildAuxDocs,
            "compile-flags" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.context(context))?; // FIXME: audit AllowPadding (before)

                // FIXME: Supported quotes arguments (they shouldn't be split in halves).
                //        Use crate `shlex` for this.
                let arguments = self.take_remaining_line().split_ascii_whitespace().collect();
                DirectiveKind::CompileFlags(arguments)
            }
            "edition" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.context(context))?; // FIXME: audit AllowPadding (before)

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let edition = self.take_while(|char| !char.is_ascii_whitespace());
                let Ok(edition) = edition.parse() else {
                    return Err(Error::new(ErrorKind::InvalidValue(edition)).context(context));
                };

                DirectiveKind::Edition(edition)
            }
            "force-host" => DirectiveKind::ForceHost,
            "no-prefer-dynamic" => DirectiveKind::NoPreferDynamic,
            "revisions" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.context(context))?; // FIXME: audit AllowPadding
                let revisions = self.take_remaining_line().split_ascii_whitespace().collect();
                DirectiveKind::Revisions(revisions)
            }
            // `compiletest` only supports a single environment variable per directive.
            "rustc-env" => {
                self.parse_separator(Padding::No).map_err(|error| error.context(context))?;
                let line = self.take_remaining_line();

                // FIXME: How does `compiletest` handle the edge cases here?
                let Some((key, value)) = line.split_once('=') else {
                    return Err(Error::new(ErrorKind::InvalidValue(line)).context(context));
                };
                DirectiveKind::RustcEnv { key, value }
            }
            "unset-rustc-env" => {
                self.parse_separator(Padding::No).map_err(|error| error.context(context))?;
                let variable = self.take_remaining_line();
                DirectiveKind::UnsetRustcEnv(variable)
            }
            // <> FIXME: Only accept these if mode==Rustdoc/Html...
            //    FIXME: Actually parse these correctly (payload...)
            "count" => DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Count, Polarity::Positive),
            "!count" => DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Count, Polarity::Negative),
            "files" => DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Files, Polarity::Positive),
            "!files" => DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Files, Polarity::Negative),
            "has" => DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Has, Polarity::Positive),
            "!has" => DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Has, Polarity::Negative),
            "has-dir" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::HasDir, Polarity::Positive)
            }
            "!has-dir" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::HasDir, Polarity::Negative)
            }
            "hasraw" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::HasRaw, Polarity::Positive)
            }
            "!hasraw" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::HasRaw, Polarity::Negative)
            }
            "matches" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Matches, Polarity::Positive)
            }
            "!matches" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Matches, Polarity::Negative)
            }
            "matchesraw" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::MatchesRaw, Polarity::Positive)
            }
            "!matchesraw" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::MatchesRaw, Polarity::Negative)
            }
            "snapshot" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Snapshot, Polarity::Positive)
            }
            "!snapshot" => {
                DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Snapshot, Polarity::Negative)
            }
            // </>
            // <> FIXME: Only accept these if mode==Rustdoc/Json...
            //    FIXME: Actually parse these correctly (payload...)
            // FIXME: actually parse directices correctly that "are both html & json"
            // "count" => DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::Count, Polarity::Positive),
            // "!count" => DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::Count, Polarity::Positive),
            // "has" => DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::Count, Polarity::Positive),
            // "!has" => DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::Count, Polarity::Positive),
            "is" => DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::Is, Polarity::Positive),
            "!is" => DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::Is, Polarity::Negative),
            "ismany" => {
                DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::IsMany, Polarity::Positive)
            }
            "!ismany" => {
                DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::IsMany, Polarity::Negative)
            }
            "set" => DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::Set, Polarity::Positive),
            "!set" => DirectiveKind::JsonDocCk(JsonDocCkDirectiveKind::Set, Polarity::Negative),
            // </>
            // NB: We don't support `{unset-,}exec-env` since it's not meaningful to rruxwry.
            directive => return Err(Error::new(ErrorKind::UnknownDirective(directive))),
        };

        Ok(Directive { revision, kind })
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

    fn expect(&mut self, expected: char) -> Result<(), Error<'src>> {
        let Some(char) = self.peek() else {
            return Err(Error::new(ErrorKind::UnexpectedEndOfInput));
        };
        if char != expected {
            return Err(Error::new(ErrorKind::UnexpectedToken { found: char, expected }));
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
        self.advance_while(|char| char.is_ascii_whitespace());
    }

    fn parse_separator(&mut self, padding: Padding) -> Result<(), Error<'src>> {
        if let Padding::Yes = padding {
            self.parse_whitespace();
        }
        self.expect(':')?;
        if let Padding::Yes = padding {
            self.parse_whitespace();
        }
        Ok(())
    }

    fn take_while(&mut self, predicate: impl Fn(char) -> bool) -> &'src str {
        if let Some(&(start, char)) = self.chars.peek() {
            let mut end = start + char.len_utf8();
            while let Some(char) = self.peek() {
                if !predicate(char) {
                    break;
                }
                if let Some((index, char)) = self.chars.next() {
                    end = index + char.len_utf8();
                }
            }
            return &self.source[start..end];
        }

        ""
    }

    fn take_remaining_line(&mut self) -> &'src str {
        self.take_while(|char| char != '\n')
    }
}

#[derive(Default)]
struct Report<'src> {
    errors: Vec<Error<'src>>,
    unknowns: Vec<&'src str>,
}

impl Report<'_> {
    fn publish(mut self) {
        if !self.unknowns.is_empty() {
            self.unknowns.sort_unstable();
            self.unknowns.dedup();

            let s = if self.unknowns.len() == 1 { "" } else { "s" };
            let unknowns =
                self.unknowns.into_iter().map(|unknown| format!("`{unknown}`")).join_with(", ");
            warning(format!("unknown directive{s}: {unknowns}")).emit();
        }

        for error in self.errors {
            // FIXME: Make `Error` impl `IntoDiagnostic`
            warning(error.to_string()).emit();
        }
    }
}

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

enum ErrorKind<'src> {
    UnknownDirective(&'src str),
    UnexpectedToken { found: char, expected: char },
    UnexpectedEndOfInput,
    InvalidValue(&'src str),
}

impl fmt::Display for ErrorKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDirective(unknown) => write!(f, "unknown directive: `{unknown}`"),
            Self::UnexpectedToken { found, expected } => {
                write!(f, "found `{found}` but expected `{expected}`")
            }
            Self::UnexpectedEndOfInput => write!(f, "unexpected end of input"),
            Self::InvalidValue(value) => write!(f, "invalid value `{value}`"),
        }
    }
}

#[derive(Clone, Copy)]
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

#[derive(Clone, Copy)]
enum Padding {
    Yes,
    No,
}
