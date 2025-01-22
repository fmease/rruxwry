//! The parser of `ui_test`-style `compiletest`, `htmldocck` and `jsondocck` directives.

// FIXME: We should warn on `//@ compile-flags:`, `//@ compile-flags`, etc.
// FIXME: Warn on `//@ revisions: single` cuz it's useless.

// FIXME: Mirror compiletest regarding `mod …`/`fn …` (sth. like that):
//        Under Flavor::Vanilla, ignore all directives below a line starts with `mod …`/`fn …`(…)
//                               and issue warnings.
//        Under Flavor::Rruxwry, don't bail out early.

// FIXME: Warn on "unused"/extraneous arguments (e.g., "//@ build-aux-docs some extra garbage").

// FIXME: Under Flavor::Rruxwry consider upgrading some(!) warnings to hard errors. If so, we might still
//        want to provide a mechanism to circumvent that. E.g., `--force` or `-S <allow|warn>=...`.

use crate::{
    command::{ExternCrate, VerbatimFlagsBuf},
    context::Context,
    data::{CrateNameRef, Edition},
    diagnostic::{EmittedError, error, fmt, warn},
    source::{LocalSpan, SourceFileRef, Span, Spanned},
    utility::{Conjunction, ListingExt, default},
};
use std::{
    collections::BTreeSet,
    mem,
    ops::{Deref, DerefMut},
    path::Path,
    str::CharIndices,
};

#[cfg(test)]
mod test;

pub(crate) fn gather<'cx>(
    path: Spanned<&Path>,
    scope: Scope,
    role: Role,
    flavor: Flavor,
    revision: Option<&str>,
    cx: Context<'cx>,
) -> crate::error::Result<Directives<'cx>> {
    // FIXME: The error handling is pretty awkward!
    let mut errors = Errors::default();
    let directives = parse(cx.map().add(path, cx)?, scope, role, flavor, &mut errors);
    // FIXME: Certain kinds of errors likely occur in large quantities (e.g., unsupported and unavailable directives).
    //        In order to avoid "terminal spamming", suppress duplicates. We actually used to *coalesce* certain
    //        error kinds but that's not super compatible with source code highlighting.
    //        Play around certain pathological inputs.
    //        ---
    //        Like, deduplication alone (" (and 5 more occurences)") doesn't help in the case where someone runs e.g.,
    //        `rrc` on an rustdoc/ test. That'll probably lead to ~4 errors getting emitted post deduplication.
    errors.emit(cx);
    directives.instantiate(revision).map_err(|error| error.emit().into())
}

fn parse<'cx>(
    file: SourceFileRef<'cx>,
    scope: Scope,
    role: Role,
    flavor: Flavor,
    errors: &mut Errors<'cx>,
) -> Directives<'cx> {
    let mut directives = Directives::new(role);

    let mut index = 0u32;
    // `\r` gets strpped as whitespace later on.
    for line in file.contents.split('\n') {
        if let Some(directive) = line.trim_start().strip_prefix("//@") {
            // FIXME: This is super awkward! Replace this!
            let offset = line.substr_range(directive).unwrap().start;
            let offset = index + file.span.start + u32::try_from(offset).unwrap();

            // FIXME: Add support for hard error, too!
            //        For example, DuplicateRevisions should lead to a hard error (unless `--force`d).
            //        Also, under Flavor::Rruxwry a lot of the warnings should become hard errors, too.
            match Parser::new(directive, scope, role, flavor, offset).parse_directive() {
                Ok(directive) => directives.add(directive),
                Err(error) => errors.insert(error),
            }
        };

        // FIXME: Is this really correct (empty lines, trailing line breaks, …)?
        index += u32::try_from(line.len()).unwrap() + 1;
    }

    // FIXME: Move this into `gather` (tests need to be updated to use a custom `gather` over `parse`).
    validate(&directives, errors);

    directives
}

fn validate<'cx>(directives: &Directives<'cx>, errors: &mut Errors<'cx>) {
    directives
        .uninstantiated
        .iter()
        .filter(|&(revision, _)| !directives.revisions.contains(revision.bare))
        .map(|&(revision, _)| Error::UndeclaredRevision {
            revision,
            available: directives.revisions.clone(),
        })
        .collect_into(errors);
}

#[derive(Clone, Copy)]
pub(crate) enum Scope {
    Base,
    HtmlDocCk,
    JsonDocCk,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum Role {
    Principal,
    Auxiliary,
}

/// The flavor of ui_test-style compiletest directives.
#[derive(Clone, Copy)]
pub(crate) enum Flavor {
    Vanilla,
    Rruxwry,
}

// FIXME: If possible get rid of the instantiated vs. uninstantiated separation.
//        Users can no longer specify multiple revisions at once, so we don't
//        need to care about "optimizing" unconditional directives.
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct Directives<'src> {
    instantiated: InstantiatedDirectives<'src>,
    uninstantiated: UninstantiatedDirectives<'src>,
    role: Role,
}

impl<'src> Directives<'src> {
    fn new(role: Role) -> Self {
        Self { instantiated: default(), uninstantiated: default(), role }
    }

    fn add(&mut self, directive: Directive<'src>) {
        if let SimpleDirective::Revisions(revisions) = directive.bare {
            // FIXME: Emit a warning for this:
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
            if let Role::Principal = self.role
                && !revisions.contains(active_revision)
            {
                return Err(InstantiationError::UndeclaredActiveRevision {
                    revision: active_revision,
                    available: revisions,
                });
            }

            for (revision, directive) in directives {
                if revision.bare == active_revision {
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
    revisions: BTreeSet<&'src str>,
}

impl<'src> InstantiatedDirectives<'src> {
    fn adjoin(&mut self, directive: SimpleDirective<'src>) {
        match directive {
            SimpleDirective::AuxBuild { path } => {
                self.dependencies.push(ExternCrate::Unnamed { path });
            }
            SimpleDirective::AuxCrate { name, path } => {
                self.dependencies
                    .push(ExternCrate::Named { name, path: path.map(|path| path.map(Into::into)) });
            }
            SimpleDirective::BuildAuxDocs => self.build_aux_docs = true,
            // These flags can indeed conflict with flags generated by us to implement other directives.
            SimpleDirective::CompileFlags(flags) => self.verbatim_flags.arguments.extend(flags),
            // FIXME: Emit an error if multiple `edition` directives were specified just like `compiletest` does.
            // FIXME:  When encountering unconditional+conditional, emit a warning.
            SimpleDirective::Edition(edition) => self.edition = Some(edition),
            SimpleDirective::Revisions(_) => unreachable!(), // Already dealt with in `Directives::add`.
            SimpleDirective::RustcEnv { key, value } => {
                self.verbatim_flags.environment.push((key, Some(value)));
            }
            SimpleDirective::UnsetRustcEnv(key) => {
                self.verbatim_flags.environment.push((key, None));
            }
            // FIXME: Actually implement these directives.
            | SimpleDirective::HtmlDocCk(..)
            | SimpleDirective::JsonDocCk(..)
            | SimpleDirective::Rruxwry(..) => {}
        }
    }
}

type UninstantiatedDirectives<'src> = Vec<(Spanned<&'src str>, SimpleDirective<'src>)>;

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
struct Directive<'src> {
    revision: Option<Spanned<&'src str>>,
    bare: SimpleDirective<'src>,
}

// FIXME: Can somehow get rid of this? By merging a few steps. This isn't super scalable rn.
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
enum SimpleDirective<'src> {
    AuxBuild {
        path: Spanned<&'src str>,
    },
    // FIXME: compiletest doesn't consider the path to be optional (gate this behind Flavor::Rruxwry).
    AuxCrate {
        name: CrateNameRef<'src>,
        path: Option<Spanned<&'src str>>,
    },
    BuildAuxDocs,
    CompileFlags(Vec<&'src str>),
    Edition(Edition),
    Revisions(Vec<&'src str>),
    RustcEnv {
        key: &'src str,
        value: &'src str,
    },
    UnsetRustcEnv(&'src str),
    #[allow(dead_code)]
    HtmlDocCk(HtmlDocCkDirective, Polarity),
    #[allow(dead_code)]
    JsonDocCk(JsonDocCkDirective, Polarity),
    #[allow(dead_code)]
    Rruxwry(RruxwryDirective),
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
pub(crate) enum RruxwryDirective {
    AuxCrateBegin,
    RawCrateBegin,
    CrateEnd,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) enum Polarity {
    Negative,
    Positive,
}

struct Parser<'src> {
    chars: CharIndices<'src>,
    #[allow(clippy::option_option)] // most natural representation in this case
    peeked: Option<Option<(usize, char)>>,
    source: &'src str,
    scope: Scope,
    role: Role,
    flavor: Flavor,
    offset: u32,
}

impl<'src> Parser<'src> {
    fn new(source: &'src str, scope: Scope, role: Role, flavor: Flavor, offset: u32) -> Self {
        Self { chars: source.char_indices(), peeked: None, source, scope, role, flavor, offset }
    }

    fn parse_directive(mut self) -> Result<Directive<'src>, Error<'src>> {
        self.parse_whitespace();

        let revision = if self.consume(|char| char == '[') {
            // FIXME: (1) Warn on empty/blank revision ("literally treated as a revision")
            //        (2) Warn on padded revision ("treated literally (not trimmed)")
            //        (3) Warn on quoted revision and commas inside the revision
            //        NOTE: In cases (1)(2) we already warn that they're undefined
            //              (it's impossible for a user to declare such revisions).
            //              However, emitting a more precise diagnostic feels nicer.
            // FIXME: Do we want to trim on Flavor::Rruxwry?
            // FIXME: Under Flavor::Rruxwry support cfg-like logic predicates here (n-ary `not`, `any`, `all`; `false`, `true`).
            let revision = self.parse_while(|char| char != ']');
            self.expect(']')?;

            Some(revision)
        } else {
            None
        };

        self.parse_whitespace();

        // FIXME: This is slightly hacky / "leaky".
        let directive =
            self.expect_many(|char| matches!(char, '-' | '!' | '}') || char.is_alphabetic())?;

        self.parse_simple_directive(directive)
            .map(|directive| Directive { revision, bare: directive })
    }

    fn parse_simple_directive(
        &mut self,
        source: Spanned<&'src str>,
    ) -> Result<SimpleDirective<'src>, Error<'src>> {
        match self.parse_base_directive(source) {
            Ok(Some(directive)) => return Ok(directive),
            Ok(None) => {}
            Err(error) => return Err(error),
        }
        let htmldocck = Self::parse_htmldocck_directive(source.bare);
        let jsondocck = Self::parse_jsondocck_directive(source.bare);
        let rruxwry = Self::parse_rruxwry_directive(source.bare);
        match (self.scope, htmldocck, jsondocck) {
            (Scope::HtmlDocCk, Some(directive), _) | (Scope::JsonDocCk, _, Some(directive)) => {
                return Ok(directive);
            }
            | (Scope::HtmlDocCk | Scope::Base, None, Some(_))
            | (Scope::JsonDocCk | Scope::Base, Some(_), None)
            | (Scope::Base, Some(_), Some(_)) => {
                // FIXME: Add more context to the error. Namely, in which scopes the directive is actually available!
                return Err(Error::UnavailableDirective(source));
            }
            _ => {}
        }
        if let Some(directive) = rruxwry {
            return match self.flavor {
                // FIXME: Add more context to the error. Namely, in which scopes the directive is actually available!
                Flavor::Vanilla => Err(Error::UnavailableDirective(source)),
                Flavor::Rruxwry => Ok(directive),
            };
        }
        Err(Error::UnknownDirective(source))
    }

    fn parse_base_directive(
        &mut self,
        source: Spanned<&'src str>,
    ) -> Result<Option<SimpleDirective<'src>>, Error<'src>> {
        Ok(Some(match source.bare {
            "aux-build" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding
                SimpleDirective::AuxBuild { path: self.parse_until_line_break() }
            }
            // `compiletest` doesn't support extern options like `priv`, `noprelude`, `nounused` or `force`
            // at the time of writing. Therefore, we don't need to deal with them here either.
            // Neither does it support optional paths (`//@ aux-crate:name`).
            // FIXME: Under Flavor::Rruxwry make the path optional.
            "aux-crate" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let name = self.expect_many(|char| char != '=' && !char.is_whitespace())?.bare;
                // FIXME: Does compiletest also validate the crate name? I doubt it.
                let Ok(name) = CrateNameRef::parse(name) else {
                    return Err(Error::InvalidValue(name));
                };

                let path = self.consume(|char| char == '=').then(|| self.parse_until_line_break());
                SimpleDirective::AuxCrate { name, path }
            }
            // FIXME: Is this available outside of rustdoc tests, too? Check compiletest's behavior!
            //        If not, intro a new scope, Scope::Rustdoc, which is separate from HtmlDocCk/JsonDocCk.
            //        Should probably be sth. akin to Scope::Rustdoc(DocCk) then.
            "build-aux-docs" => SimpleDirective::BuildAuxDocs,
            "compile-flags" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding (before)

                // FIXME: Supported quotes arguments (they shouldn't be split in halves).
                //        Use crate `shlex` for this.
                let arguments = self.parse_until_line_break().bare.split_whitespace().collect();
                SimpleDirective::CompileFlags(arguments)
            }
            "edition" => {
                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding (before)

                // We're doing this two-step process — (greedy) lexing followed by validation —
                // to be able to provide a better error message.
                let edition = self.expect_many(|char| !char.is_whitespace())?.bare;
                // FIXME: Don't actually try to parse the edition!
                let Ok(edition) = edition.parse() else {
                    return Err(Error::InvalidValue(edition));
                };

                SimpleDirective::Edition(edition)
            }
            // FIXME: Warn/error if we're inside of an auxiliary file.
            //        ->Warn: "directive gets ignored // revisions are inherited in aux"
            //        ->Error: "directive not permitted // revisions are inherited in aux"
            //        For that, introduce a new parameter: PermitRevisionDeclarations::{Yes, No}.
            "revisions" => {
                // FIXME: Add a unit test for this!
                if let Role::Auxiliary = self.role {
                    return Err(Error::AuxiliaryRevisionDeclaration(source.span));
                }

                self.parse_separator(Padding::Yes)?; // FIXME: audit AllowPadding
                let line = self.parse_until_line_break();
                let mut revisions: Vec<_> = line.bare.split_whitespace().collect();
                let count = revisions.len();
                revisions.sort_unstable();
                revisions.dedup();
                if count != revisions.len() {
                    // FIXME: Provide a more precise message and span.
                    return Err(Error::DuplicateRevisions(line.span));
                }
                SimpleDirective::Revisions(revisions)
            }
            // `compiletest` only supports a single environment variable per directive.
            "rustc-env" => {
                self.parse_separator(Padding::No)?;
                let line = self.parse_until_line_break().bare;

                // FIXME: How does `compiletest` handle the edge cases here?
                let Some((key, value)) = line.split_once('=') else {
                    return Err(Error::InvalidValue(line));
                };
                SimpleDirective::RustcEnv { key, value }
            }
            "unset-rustc-env" => {
                self.parse_separator(Padding::No)?;
                let variable = self.parse_until_line_break().bare;
                SimpleDirective::UnsetRustcEnv(variable)
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
            _ if source.bare.starts_with("ignore-")
                || source.bare.starts_with("needs-")
                || source.bare.starts_with("only-") =>
            {
                return Err(Error::UnsupportedDirective(source));
            }
            _ => return Ok(None),
        }))
    }

    // FIXME: Actually parse them fully and do sth. with them, otherwise turn this into a array lookup.
    fn parse_htmldocck_directive(source: &'src str) -> Option<SimpleDirective<'src>> {
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
            _ => return None,
        };
        Some(SimpleDirective::HtmlDocCk(directive, polarity))
    }

    // FIXME: Actually parse them fully and do sth. with them, otherwise turn this into a array lookup.
    fn parse_jsondocck_directive(source: &'src str) -> Option<SimpleDirective<'src>> {
        let (source, polarity) = Self::parse_polarity(source);
        let directive = match source {
            "count" => JsonDocCkDirective::Count,
            "has" => JsonDocCkDirective::Has,
            "is" => JsonDocCkDirective::Is,
            "ismany" => JsonDocCkDirective::IsMany,
            "set" => JsonDocCkDirective::Set,
            _ => return None,
        };
        Some(SimpleDirective::JsonDocCk(directive, polarity))
    }

    // FIXME: Actually parse them fully.
    fn parse_rruxwry_directive(source: &'src str) -> Option<SimpleDirective<'src>> {
        let directive = match source {
            "crate" => RruxwryDirective::AuxCrateBegin,
            "raw-crate" => RruxwryDirective::RawCrateBegin,
            "}" => RruxwryDirective::CrateEnd,
            _ => return None,
        };
        Some(SimpleDirective::Rruxwry(directive))
    }

    fn parse_polarity(source: &'src str) -> (&'src str, Polarity) {
        match source.strip_prefix('!') {
            Some(source) => (source, Polarity::Negative),
            None => (source, Polarity::Positive),
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.peeked.get_or_insert_with(|| self.chars.next()).map(|(_, char)| char)
    }

    fn advance(&mut self) {
        if self.peeked.take().is_none() {
            self.chars.next();
        }
    }

    fn index(&self) -> u32 {
        match self.peeked {
            Some(Some((index, _))) => index,
            _ => self.chars.offset(),
        }
        .try_into()
        .unwrap()
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
            return Err(Error::UnexpectedEndOfInput(self.span(LocalSpan::empty(self.index()))));
        };
        if char != expected {
            let span = self.span(LocalSpan::with_len(self.index(), len_utf8(char)));
            return Err(Error::UnexpectedToken { found: Spanned::new(span, char), expected });
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

    fn expect_many(
        &mut self,
        predicate: impl Fn(char) -> bool,
    ) -> Result<Spanned<&'src str>, Error<'src>> {
        let result = self.parse_while(predicate);
        if result.span.is_empty() {
            return Err(Error::UnexpectedEndOfInput(result.span));
        }
        Ok(result)
    }

    fn parse_while(&mut self, predicate: impl Fn(char) -> bool) -> Spanned<&'src str> {
        let mut span = LocalSpan::empty(self.index());

        while let Some(char) = self.peek() {
            if !predicate(char) {
                break;
            }
            span.end += len_utf8(char);
            self.advance();
        }

        self.spanned(span)
    }

    // FIXME: Should we bail on empty lines?
    fn parse_until_line_break(&mut self) -> Spanned<&'src str> {
        self.parse_while(|char| char != '\n')
    }

    fn spanned(&self, span: LocalSpan) -> Spanned<&'src str> {
        Spanned::new(self.span(span), self.source(span))
    }

    fn source(&self, span: LocalSpan) -> &'src str {
        &self.source[span.range()]
    }

    fn span(&self, span: LocalSpan) -> Span {
        // FIXME: Consider utilizing (the currently unused) `Span::global`
        //        here once we no longer need to shift by offset.
        span.shift(self.offset).reinterpret()
    }
}

#[allow(clippy::cast_possible_truncation)] // False positive, `len_utf8`'s result is always in 0..=4.
fn len_utf8(char: char) -> u32 {
    char.len_utf8() as _
}

#[derive(Default)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
struct Errors<'src>(Vec<Error<'src>>);

impl<'src> Errors<'src> {
    fn insert(&mut self, error: Error<'src>) {
        self.0.push(error);
    }

    // FIXME: Shouldn't all these errors be emitted as (non-fatal) errors instead of warnings?
    fn emit(self, cx: Context<'_>) {
        self.0.into_iter().for_each(|error| error.emit(cx));
    }
}

impl<'src> Extend<Error<'src>> for Errors<'src> {
    fn extend<T: IntoIterator<Item = Error<'src>>>(&mut self, errors: T) {
        errors.into_iter().for_each(|error| self.insert(error));
    }
}

impl Error<'_> {
    // FIXME: Equip all of these errors with source locations!
    fn emit(self, cx: Context<'_>) {
        // FIXME: Improve the phrasing of these diagnostics!
        match self {
            Self::UnavailableDirective(name) => {
                warn(fmt!("unavailable directive: `{name}`")).highlight(name.span, cx)
            }
            Self::UnsupportedDirective(name) => {
                warn(fmt!("unsupported directive: `{name}`")).highlight(name.span, cx)
            }
            Self::UnknownDirective(name) => {
                warn(fmt!("unknown directive: `{name}`")).highlight(name.span, cx)
            }
            Self::UnexpectedToken { found, expected } => {
                error(fmt!("found `{found}` but expected `{expected}`")).highlight(found.span, cx)
            }
            Self::UnexpectedEndOfInput(span) => {
                error(fmt!("unexpected end of input")).highlight(span, cx)
            }
            // FIXME: Source span!
            Self::InvalidValue(value) => error(fmt!("invalid value `{value}`")),
            Self::DuplicateRevisions(span) => {
                error(fmt!("duplicate revisions")).highlight(span, cx)
            }
            Self::UndeclaredRevision { revision, available } => {
                // FIXME: Dedupe w/ InstErr:
                let list = |available: BTreeSet<_>| {
                    available
                        .into_iter()
                        .map(|revision| format!("`{revision}`"))
                        .list(Conjunction::And)
                };

                let it =
                    error(fmt!("undeclared revision `{revision}`")).highlight(revision.span, cx);

                if available.is_empty() {
                    it.help(fmt!("consider declaring a revision with the `revisions` directive"))
                } else {
                    // FIXME: Also add the help here to extend the revisisons directive
                    it.note(fmt!("available revisions are: {}", list(available)))
                }
            }
            Self::AuxiliaryRevisionDeclaration(span) => {
                error(fmt!("revision declaration in auxiliary file"))
                    .highlight(span, cx)
                    .note(fmt!("declared revisions are inherited from the principal file"))
            }
        }
        .finish();
    }
}

#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
enum Error<'src> {
    UnavailableDirective(Spanned<&'src str>),
    UnsupportedDirective(Spanned<&'src str>),
    UnknownDirective(Spanned<&'src str>),
    UnexpectedToken { found: Spanned<char>, expected: char },
    UnexpectedEndOfInput(Span),
    InvalidValue(&'src str),
    DuplicateRevisions(Span),
    UndeclaredRevision { revision: Spanned<&'src str>, available: BTreeSet<&'src str> },
    AuxiliaryRevisionDeclaration(Span),
}

// FIXME: Get rid of this if possible.
#[derive(Clone, Copy)]
enum Padding {
    Yes,
    No,
}
