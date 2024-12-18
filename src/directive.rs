//! The parser of `ui_test`-style `compiletest`, `htmldocck` and `jsondocck` directives.

// FIXME: What does compiletest do for `//@ revisions: off` `//@[off] undefined`? We warn.
// FIXME: Does compiletest permit `//@[pre] check-pass` `//@ revisions: pre`?
// FIXME: We should warn on `//@[undeclared] compile-flags:`.
// FIXME: What does compiletest do on `//@ revisions: dupe dupe`? We should warn.

use crate::{
    command::{ExternCrate, VerbatimFlagsBuf},
    data::{CrateNameRef, Edition},
    diagnostic::warning,
    utility::{default, parse},
};
use ra_ap_rustc_lexer::TokenKind;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    iter::Peekable,
    ops::{Deref, DerefMut},
    str::CharIndices,
};

pub(crate) fn parse<'src>(source: &'src str, scope: Scope) -> Directives<'src> {
    let mut buffer = ErrorBuffer::default();
    let mut parser = parse::SourceFileParser::new(source);
    let mut directives = Directives::default();

    // FIXME: Does compiletest actually rust-tokenize the input? I doubt it.
    //        Simplify this once you've confirmed that.
    while let Some(token) = parser.peek() {
        if let TokenKind::LineComment { doc_style: None } = token.kind
            && let comment = parser.source()
            && let Some(directive) = comment.strip_prefix("//@")
        {
            match Directive::parse(directive, scope) {
                Ok(directive) => directives.add(directive),
                Err(error) => buffer.add(error),
            };
        }

        parser.advance();
    }

    buffer.release();
    directives
}

#[derive(Clone, Copy)]
pub(crate) enum Scope {
    Base,
    HtmlDocCk,
    JsonDocCk,
}

#[derive(Default)]
pub(crate) struct Directives<'src> {
    instantiated: InstantiatedDirectives<'src>,
    uninstantiated: UninstantiatedDirectives<'src>,
}

impl<'src> Directives<'src> {
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
    pub(crate) fn into_instantiated(mut self, revs: &BTreeSet<&str>) -> Self {
        let uninstantiated = std::mem::take(&mut self.uninstantiated);
        Self::instantiate(&mut self, &uninstantiated, revs);
        self
    }

    /// Instantiate all directives that are conditional on a revision.
    #[allow(dead_code)] // FIXME: use this when impl'ing `--all-revs`
    pub(crate) fn instantiated(&self, revs: &BTreeSet<&str>) -> Self {
        let mut instantiated =
            Self { instantiated: self.instantiated.clone(), uninstantiated: default() };
        Self::instantiate(&mut instantiated, &self.uninstantiated, revs);
        instantiated
    }

    fn instantiate(
        instantiated: &mut InstantiatedDirectives<'src>,
        uninstantiated: &UninstantiatedDirectives<'src>,
        revisions: &BTreeSet<&str>,
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

impl DerefMut for Directives<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.instantiated
    }
}

#[derive(Default, Clone)]
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
            //        just like `compiletest` does.
            DirectiveKind::Edition(edition) => self.edition = Some(edition),
            DirectiveKind::ForceHost => self.force_host = true,
            DirectiveKind::NoPreferDynamic => self.no_prefer_dynamic = true,
            DirectiveKind::Revisions(_) => unreachable!(), // Already dealt with in `Self::add`.
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

type UninstantiatedDirectives<'src> = BTreeMap<&'src str, Vec<DirectiveKind<'src>>>;

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
    scope: Scope,
}

impl<'src> DirectiveParser<'src> {
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
        let htmldocck = match self.parse_htmldocck_directive(source) {
            Ok(directive) => Some(directive),
            Err(ErrorKind::UnknownDirective(_)) => None,
            Err(error) => return Err(Error::new(error).context(context)),
        };
        let jsondocck = match self.parse_jsondocck_directive(source) {
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
        // FIXME: Don't do the error.kind extraction.
        //        Instead, parse functions should return ErrorKind (→BareError) instead of Error

        Ok(match source {
            "aux-build" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.kind)?; // FIXME: audit AllowPadding
                let path = self.take_remaining_line();
                DirectiveKind::AuxBuild { path }
            }
            // `compiletest` doesn't support extern options like `priv`, `noprelude`, `nounused` or `force`
            // at the time of writing. Therefore, we don't need to deal with them here either.
            // Neither does it support optional paths (`//@ aux-crate:name`).
            "aux-crate" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.kind)?; // FIXME: audit AllowPadding

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let name = self.take_while(|char| char != '=' && !char.is_ascii_whitespace());
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
                self.parse_separator(Padding::Yes).map_err(|error| error.kind)?; // FIXME: audit AllowPadding (before)

                // FIXME: Supported quotes arguments (they shouldn't be split in halves).
                //        Use crate `shlex` for this.
                let arguments = self.take_remaining_line().split_ascii_whitespace().collect();
                DirectiveKind::CompileFlags(arguments)
            }
            "edition" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.kind)?; // FIXME: audit AllowPadding (before)

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let edition = self.take_while(|char| !char.is_ascii_whitespace());
                let Ok(edition) = edition.parse() else {
                    return Err(ErrorKind::InvalidValue(edition));
                };

                DirectiveKind::Edition(edition)
            }
            "force-host" => DirectiveKind::ForceHost,
            "no-prefer-dynamic" => DirectiveKind::NoPreferDynamic,
            "revisions" => {
                self.parse_separator(Padding::Yes).map_err(|error| error.kind)?; // FIXME: audit AllowPadding
                let revisions = self.take_remaining_line().split_ascii_whitespace().collect();
                DirectiveKind::Revisions(revisions)
            }
            // `compiletest` only supports a single environment variable per directive.
            "rustc-env" => {
                self.parse_separator(Padding::No).map_err(|error| error.kind)?;
                let line = self.take_remaining_line();

                // FIXME: How does `compiletest` handle the edge cases here?
                let Some((key, value)) = line.split_once('=') else {
                    return Err(ErrorKind::InvalidValue(line));
                };
                DirectiveKind::RustcEnv { key, value }
            }
            "unset-rustc-env" => {
                self.parse_separator(Padding::No).map_err(|error| error.kind)?;
                let variable = self.take_remaining_line();
                DirectiveKind::UnsetRustcEnv(variable)
            }
            // FIXME: proc-macro, exec-env, unset-exec-env, run-flags, doc-flags
            source => return Err(ErrorKind::UnknownDirective(source)),
        })
    }

    // FIXME: Actually parse them fully and do sth. with them, otherwise turn this into a array lookup.
    fn parse_htmldocck_directive(
        &self,
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
        &self,
        source: &'src str,
    ) -> Result<DirectiveKind<'src>, ErrorKind<'src>> {
        let (source, polarity) = Self::parse_polarity(source);
        let kind = match source {
            "count" => JsonDocCkDirectiveKind::Count,
            "has" => JsonDocCkDirectiveKind::Count,
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
        let list = |message: &mut String, mut elements: BTreeSet<_>| {
            use std::fmt::Write as _;

            if let Some(element) = elements.pop_first() {
                write!(message, "`{element}`").unwrap();
            }
            for element in elements {
                write!(message, ", `{element}`").unwrap();
            }
        };

        if !self.unknowns.is_empty() {
            let s = if self.unknowns.len() == 1 { "" } else { "s" };
            let mut message = format!("unknown directive{s}: ");
            list(&mut message, self.unknowns);
            warning(message).emit();
        }

        if !self.unavailables.is_empty() {
            let s = if self.unavailables.len() == 1 { "" } else { "s" };
            // FIXME: Better error message.
            let mut message = format!("unavailable directive{s}: ");
            list(&mut message, self.unavailables);
            warning(message).emit();
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
    // FIXME: add scope?
    UnavailableDirective(&'src str),
    UnexpectedToken { found: char, expected: char },
    UnexpectedEndOfInput,
    InvalidValue(&'src str),
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
