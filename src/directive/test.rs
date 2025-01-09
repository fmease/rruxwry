use super::*;
use crate::utility::default;

// FIXME: Test trailing colons `:` (1) on argument-taking directives (2) on argument-less directivess.
// FIXME: Test padded colons `  :  `.
// FIXME: Test `revisions: one, two, three` (what does compiletest do??).
// FIXME: Test non-alphanum revision "names" in `revisions` directive (what does compiletest do??).
// FIXME: Test shell-escaping for compile-flags etc once the parser supports it
// FIXME: Test CRLF.

fn parse_directives<'cx>(
    source: &'cx str,
    scope: Scope,
    flavor: Flavor,
    errors: &mut Errors<'cx>,
) -> Directives<'cx> {
    parse(
        SourceFileRef { path: Path::new(""), contents: source, span: Span::SHAM },
        scope,
        flavor,
        errors,
    )
}

fn parse_directive(source: &str, scope: Scope) -> Result<Directive<'_>, Error<'_>> {
    // FIXME: Make flavor a parameter.
    // FIXME: Actually set the offset to 1 from 0 make room for Span::SHAM.
    Parser::new(source, scope, Flavor::Vanilla, 0).parse_directive()
}

fn span(start: u32, end: u32) -> Span {
    Span { start, end }
}

fn spanned<T>(start: u32, end: u32, bare: T) -> Spanned<T> {
    Spanned::new(span(start, end), bare)
}

#[test]
fn empty_directive() {
    assert_eq!(parse_directive("", Scope::Base), Err(Error::UnexpectedEndOfInput));
}

#[test]
fn blank_directive() {
    assert_eq!(parse_directive("  \t   ", Scope::Base), Err(Error::UnexpectedEndOfInput));
}

#[test]
fn unavailable_htmldocck_directive_base() {
    assert_eq!(
        parse_directive("!has", Scope::Base),
        Err(Error::UnavailableDirective(spanned(0, 4, "!has")))
    );
}

#[test]
fn unavailable_rruxwry_directive_base() {
    assert_eq!(
        parse_directive("crate inner {", Scope::Base),
        Err(Error::UnavailableDirective(spanned(0, 5, "crate")))
    );
}

#[test]
fn unavailable_jsondocck_directive_htmldocck() {
    assert_eq!(
        parse_directive("is", Scope::HtmlDocCk),
        Err(Error::UnavailableDirective(spanned(0, 2, "is")))
    );
}

#[test]
fn unsupported_directive() {
    assert_eq!(
        parse_directive("check-pass", Scope::Base),
        Err(Error::UnsupportedDirective(spanned(0, 10, "check-pass")))
    );
}

#[test]
fn unknown_directive() {
    assert_eq!(
        parse_directive("  undefined ", Scope::Base),
        Err(Error::UnknownDirective(spanned(2, 11, "undefined",)))
    );
}

#[test]
fn build_aux_docs_directive() {
    assert_eq!(
        parse_directive("build-aux-docs", Scope::Base),
        Ok(Directive { revision: None, bare: SimpleDirective::BuildAuxDocs })
    );
}

#[test]
fn htmldocck_directive() {
    assert_eq!(
        parse_directive("has 'krate/constant.K.html'", Scope::HtmlDocCk),
        Ok(Directive {
            revision: None,
            bare: SimpleDirective::HtmlDocCk(HtmlDocCkDirective::Has, Polarity::Positive),
        })
    );
}

#[test]
fn edition_directive_no_colon() {
    // FIXME: This should only warn (under -@) and discard the whole directive
    assert_eq!(
        parse_directive("edition 2018", Scope::Base),
        Err(Error::UnexpectedToken { found: spanned(8, 9, '2'), expected: ':' })
    );
}

#[test]
fn revisions_directive() {
    assert_eq!(
        parse_directive("revisions: one \ttwo  three", Scope::Base),
        Ok(Directive {
            revision: None,
            bare: SimpleDirective::Revisions(vec!["one", "three", "two"])
        })
    );
}

#[test]
fn revisions_directive_duplicate_revisions() {
    assert_eq!(
        parse_directive("revisions: repeat repeat", Scope::Base),
        Err(Error::DuplicateRevisions(span(11, 24)))
    );
}

#[test]
fn conditional_directive() {
    assert_eq!(
        parse_directive("[rev] aux-build: file.rs", Scope::Base),
        Ok(Directive {
            revision: Some(spanned(1, 4, "rev")),
            bare: SimpleDirective::AuxBuild { path: spanned(17, 24, "file.rs") }
        })
    );
}

#[test]
fn empty_conditional_directive() {
    assert_eq!(parse_directive("[predicate]", Scope::Base), Err(Error::UnexpectedEndOfInput));
}

#[test]
fn unknown_conditional_directive() {
    assert_eq!(
        parse_directive("[whatever] unknown", Scope::Base),
        Err(Error::UnknownDirective(spanned(11, 18, "unknown")))
    );
}

#[test]
fn empty_revision() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive("[] edition: 2021", Scope::Base),
        Ok(Directive {
            revision: Some(spanned(0, 0, "")), // FIXME: placeholder span
            bare: SimpleDirective::Edition(Edition::Rust2021)
        })
    );
}

#[test]
fn padded_revision_not_trimmed() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive(" [  padded \t] edition: 2015", Scope::Base),
        Ok(Directive {
            revision: Some(spanned(2, 12, "  padded \t")),
            bare: SimpleDirective::Edition(Edition::Rust2015)
        })
    );
}

#[test]
fn quoted_revision_not_unquoted() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive("[\"literally\"] compile-flags:", Scope::Base),
        Ok(Directive {
            revision: Some(spanned(1, 12, "\"literally\"")),
            bare: SimpleDirective::CompileFlags(Vec::new())
        })
    );
}

#[test]
fn commas_inside_revision() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive("[one,two] compile-flags:", Scope::Base),
        Ok(Directive {
            revision: Some(spanned(1, 8, "one,two")),
            bare: SimpleDirective::CompileFlags(Vec::new())
        })
    );
}

// FIXME: This should probably be a `directive::try_parse` test
#[test]
fn conditional_directives_directive() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive("[recur] revisions: recur", Scope::Base),
        Ok(Directive {
            revision: Some(spanned(1, 6, "recur")),
            bare: SimpleDirective::Revisions(vec!["recur"])
        })
    );
}

#[test]
fn unterminated_revision_condition() {
    assert_eq!(parse_directive("[half-open", Scope::Base), Err(Error::UnexpectedEndOfInput));
}

#[test]
fn no_directives() {
    let mut errors = Errors::default();
    let directives = parse_directives(
        "#![crate_type = \"lib\"]\nfn main() {}\n",
        Scope::Base,
        Flavor::Vanilla,
        &mut errors,
    );
    assert_eq!((directives, errors), default());
}

#[test]
fn compile_flags_directives() {
    let mut errors = Errors::default();
    let directives = parse_directives(
        "\n  \t  //@  compile-flags: --crate-type lib\n\
        //@compile-flags:--edition=2021",
        Scope::Base,
        Flavor::Vanilla,
        &mut errors,
    );
    assert_eq!(directives, Directives {
        instantiated: InstantiatedDirectives {
            verbatim_flags: VerbatimFlagsBuf {
                arguments: vec!["--crate-type", "lib", "--edition=2021"],
                ..default()
            },
            ..default()
        },
        uninstantiated: default(),
    });
    assert_eq!(errors, default());
}

#[test]
fn conditional_directives() {
    let mut errors = Errors::default();
    let directives = parse_directives(
        "//@ revisions: one two\n\
         //@[one] edition: 2018\n\
         //@ compile-flags: --crate-type=lib\n\
         //@[two] compile-flags: -Zparse-crate-root-only",
        Scope::Base,
        Flavor::Vanilla,
        &mut errors,
    );
    assert_eq!(directives, Directives {
        instantiated: InstantiatedDirectives {
            revisions: ["one", "two"].into(),
            verbatim_flags: VerbatimFlagsBuf { arguments: vec!["--crate-type=lib"], ..default() },
            ..default()
        },
        uninstantiated: vec![
            (spanned(27, 30, "one"), SimpleDirective::Edition(Edition::Rust2018)),
            (
                spanned(86, 89, "two"),
                SimpleDirective::CompileFlags(vec!["-Zparse-crate-root-only"])
            )
        ]
    });
    assert_eq!(errors, default());
}

#[test]
fn instantiate_conditional_directives() {
    let mut errors = Errors::default();
    let directives = parse_directives(
        "//@ revisions: one two\n\
         //@[one] edition: 2018\n\
         //@ compile-flags: --crate-type=lib\n\
         //@[two] compile-flags: -Zparse-crate-root-only",
        Scope::Base,
        Flavor::Vanilla,
        &mut errors,
    )
    .instantiate(Some("two"));
    assert_eq!(errors, default());
    assert_eq!(
        directives,
        Ok(Directives {
            instantiated: InstantiatedDirectives {
                revisions: default(),
                verbatim_flags: VerbatimFlagsBuf {
                    arguments: vec!["--crate-type=lib", "-Zparse-crate-root-only"],
                    ..default()
                },
                ..default()
            },
            uninstantiated: default()
        })
    );
}

#[test]
fn conditional_directives_revision_declared_after_use() {
    let mut errors = Errors::default();
    let directives = parse_directives(
        "//@[next] compile-flags: -Znext-solver\n\
         //@ revisions: classic next",
        Scope::Base,
        Flavor::Vanilla,
        &mut errors,
    );
    assert_eq!(directives, Directives {
        instantiated: InstantiatedDirectives { revisions: ["classic", "next"].into(), ..default() },
        uninstantiated: vec![(
            spanned(4, 8, "next"),
            SimpleDirective::CompileFlags(vec!["-Znext-solver"])
        )],
    });
    assert_eq!(errors, default());
}

#[test]
fn conditional_directives_undeclared_revisions() {
    let mut errors = Errors::default();
    let directives = parse_directives(
        "//@[block] compile-flags: --crate-type lib\n\
         //@[wall] edition: 2021",
        Scope::Base,
        Flavor::Vanilla,
        &mut errors,
    );
    assert_eq!(directives, Directives {
        instantiated: default(),
        uninstantiated: vec![
            (spanned(4, 9, "block"), SimpleDirective::CompileFlags(vec!["--crate-type", "lib"])),
            (spanned(47, 51, "wall"), SimpleDirective::Edition(Edition::Rust2021)),
        ]
    });
    assert_eq!(
        errors,
        Errors(vec![
            Error::UndeclaredRevision { revision: spanned(4, 9, "block"), available: default() },
            Error::UndeclaredRevision { revision: spanned(47, 51, "wall"), available: default() },
        ])
    );
}

#[test]
fn undeclared_active_revision() {
    assert_eq!(
        Directives::default().instantiate(Some("flag")),
        Err(InstantiationError::UndeclaredActiveRevision {
            revision: "flag",
            available: default()
        })
    );
}

#[test]
fn missing_active_revision() {
    let mut errors = Errors::default();
    let directives =
        parse_directives("//@ revisions: first second", Scope::Base, Flavor::Vanilla, &mut errors);
    assert_eq!(errors, default());
    assert_eq!(
        directives.instantiate(None),
        Err(InstantiationError::MissingActiveRevision { available: ["first", "second"].into() })
    );
}
