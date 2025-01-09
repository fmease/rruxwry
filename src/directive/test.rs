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
        SourceFileRef { path: Path::new(""), contents: source, span: Span { start: 0, end: 0 } },
        scope,
        flavor,
        errors,
    )
}

fn parse_directive(source: &str, scope: Scope) -> Result<Directive<'_>, Error<'_>> {
    // FIXME: Make flavor a parameter.
    Parser::new(source, scope, Flavor::Vanilla, 0).parse_directive()
}

fn span(start: u32, end: u32) -> Span {
    Span { start, end }
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
        Err(Error::UnavailableDirective("!has", span(0, 4)))
    );
}

#[test]
fn unavailable_rruxwry_directive_base() {
    assert_eq!(
        parse_directive("crate inner {", Scope::Base),
        Err(Error::UnavailableDirective("crate", span(0, 5)))
    );
}

#[test]
fn unavailable_jsondocck_directive_htmldocck() {
    assert_eq!(
        parse_directive("is", Scope::HtmlDocCk),
        Err(Error::UnavailableDirective("is", span(0, 2)))
    );
}

#[test]
fn unsupported_directive() {
    assert_eq!(
        parse_directive("check-pass", Scope::Base),
        Err(Error::UnsupportedDirective("check-pass", span(0, 10)))
    );
}

#[test]
fn unknown_directive() {
    assert_eq!(
        parse_directive("  undefined ", Scope::Base),
        Err(Error::UnknownDirective("undefined", span(2, 11)))
    );
}

#[test]
fn build_aux_docs_directive() {
    assert_eq!(
        parse_directive("build-aux-docs", Scope::Base),
        Ok(Directive { revision: None, bare: BareDirective::BuildAuxDocs })
    );
}

#[test]
fn htmldocck_directive() {
    assert_eq!(
        parse_directive("has 'krate/constant.K.html'", Scope::HtmlDocCk),
        Ok(Directive {
            revision: None,
            bare: BareDirective::HtmlDocCk(HtmlDocCkDirective::Has, Polarity::Positive),
        })
    );
}

#[test]
fn edition_directive_no_colon() {
    // FIXME: This should only warn (under -@) and discard the whole directive
    assert_eq!(
        parse_directive("edition 2018", Scope::Base),
        Err(Error::UnexpectedToken { found: ('2', span(8, 9)), expected: ':' })
    );
}

#[test]
fn revisions_directive() {
    assert_eq!(
        parse_directive("revisions: one \ttwo  three", Scope::Base),
        Ok(Directive {
            revision: None,
            bare: BareDirective::Revisions(vec!["one", "three", "two"])
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
            revision: Some(("rev", span(1, 4))),
            bare: BareDirective::AuxBuild { path: ("file.rs", span(17, 24)) }
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
        Err(Error::UnknownDirective("unknown", span(11, 18)))
    );
}

#[test]
fn empty_revision() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive("[] edition: 2021", Scope::Base),
        Ok(Directive {
            revision: Some(("", span(0, 0))),
            bare: BareDirective::Edition(Edition::Rust2021)
        })
    );
}

#[test]
fn padded_revision_not_trimmed() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive(" [  padded \t] edition: 2015", Scope::Base),
        Ok(Directive {
            revision: Some(("  padded \t", span(2, 12))),
            bare: BareDirective::Edition(Edition::Rust2015)
        })
    );
}

#[test]
fn quoted_revision_not_unquoted() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive("[\"literally\"] compile-flags:", Scope::Base),
        Ok(Directive {
            revision: Some(("\"literally\"", span(1, 12))),
            bare: BareDirective::CompileFlags(Vec::new())
        })
    );
}

#[test]
fn commas_inside_revision() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        parse_directive("[one,two] compile-flags:", Scope::Base),
        Ok(Directive {
            revision: Some(("one,two", span(1, 8))),
            bare: BareDirective::CompileFlags(Vec::new())
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
            revision: Some(("recur", span(1, 6))),
            bare: BareDirective::Revisions(vec!["recur"])
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
            (("one", span(27, 30)), BareDirective::Edition(Edition::Rust2018)),
            (("two", span(86, 89)), BareDirective::CompileFlags(vec!["-Zparse-crate-root-only"]))
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
            ("next", span(4, 8)),
            BareDirective::CompileFlags(vec!["-Znext-solver"])
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
            (("block", span(4, 9)), BareDirective::CompileFlags(vec!["--crate-type", "lib"])),
            (("wall", span(47, 51)), BareDirective::Edition(Edition::Rust2021)),
        ]
    });
    assert_eq!(
        errors,
        Errors(vec![
            Error::UndeclaredRevision { revision: ("block", span(4, 9)), available: default() },
            Error::UndeclaredRevision { revision: ("wall", span(47, 51)), available: default() },
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
