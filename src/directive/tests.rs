use super::*;
use crate::utility::default;

// FIXME: Test trailing colons `:` (1) on argument-taking directives (2) on argument-less directivess.
// FIXME: Test padded colons `  :  `.
// FIXME: Test `revisions: one, two, three` (what does compiletest do??).
// FIXME: Test non-alphanum revision "names" in `revisions` directive (what does compiletest do??).
// FIXME: Add `conditional_directive*s*`
// FIXME: Test shell-escaping for compile-flags etc once the parser support it

#[test]
fn empty_directive() {
    assert_eq!(Directive::parse("", Scope::Base), Err(Error::new(ErrorKind::UnexpectedEndOfInput)));
}

#[test]
fn whitespace_only_directive() {
    assert_eq!(
        Directive::parse("  \t   ", Scope::Base),
        Err(Error::new(ErrorKind::UnexpectedEndOfInput))
    )
}

#[test]
fn unknown_directive() {
    assert_eq!(
        Directive::parse("  undefined ", Scope::Base),
        Err(Error::new(ErrorKind::UnknownDirective("undefined")))
    )
}

#[test]
fn unavailable_directive_base() {
    assert_eq!(
        Directive::parse("!has", Scope::Base),
        Err(Error::new(ErrorKind::UnavailableDirective("!has")))
    );
}

#[test]
fn build_aux_docs_directive() {
    assert_eq!(
        Directive::parse("build-aux-docs", Scope::Base),
        Ok(Directive { revision: None, kind: DirectiveKind::BuildAuxDocs })
    )
}

#[test]
fn htmldocck_directive() {
    assert_eq!(
        Directive::parse("has 'krate/constant.K.html'", Scope::HtmlDocCk),
        Ok(Directive {
            revision: None,
            kind: DirectiveKind::HtmlDocCk(HtmlDocCkDirectiveKind::Has, Polarity::Positive),
        })
    );
}

#[test]
fn edition_directive_no_colon() {
    // FIXME: This should only warn (under -@) and discard the whole directive
    assert_eq!(
        Directive::parse("edition 2018", Scope::Base),
        Err(Error::new(ErrorKind::UnexpectedToken { found: '2', expected: ':' })
            .context(ErrorContext::Directive("edition")))
    );
}

#[test]
fn revisions_directive() {
    assert_eq!(
        Directive::parse("revisions: one \ttwo  three", Scope::Base),
        Ok(Directive {
            revision: None,
            kind: DirectiveKind::Revisions(vec!["one", "three", "two"])
        })
    );
}

#[test]
fn revisions_directive_duplicate_revisions() {
    // FIXME: The error context is redundant here.
    assert_eq!(
        Directive::parse("revisions: repeat repeat", Scope::Base),
        Err(Error::new(ErrorKind::DuplicateRevisions).context(ErrorContext::Directive("revisions")))
    );
}

#[test]
fn conditional_directive() {
    assert_eq!(
        Directive::parse("[rev] aux-build: file.rs", Scope::Base),
        Ok(Directive { revision: Some("rev"), kind: DirectiveKind::AuxBuild { path: "file.rs" } })
    );
}

#[test]
fn empty_conditional_directive() {
    assert_eq!(
        Directive::parse("[predicate]", Scope::Base),
        Err(Error::new(ErrorKind::UnexpectedEndOfInput))
    );
}

#[test]
fn unknown_conditional_directive() {
    assert_eq!(
        Directive::parse("[whatever] unknown", Scope::Base),
        Err(Error::new(ErrorKind::UnknownDirective("unknown")))
    );
}

#[test]
fn empty_revision() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        Directive::parse("[] edition: 2021", Scope::Base),
        Ok(Directive { revision: Some(""), kind: DirectiveKind::Edition(Edition::Rust2021) })
    );
}

#[test]
fn padded_revision_not_trimmed() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        Directive::parse(" [  padded \t] edition: 2015", Scope::Base),
        Ok(Directive {
            revision: Some("  padded \t"),
            kind: DirectiveKind::Edition(Edition::Rust2015)
        })
    );
}

#[test]
fn quoted_revision_not_unquoted() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        Directive::parse("[\"literally\"] compile-flags:", Scope::Base),
        Ok(Directive {
            revision: Some("\"literally\""),
            kind: DirectiveKind::CompileFlags(Vec::new())
        })
    )
}

#[test]
fn commas_inside_revision() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        Directive::parse("[one,two] compile-flags:", Scope::Base),
        Ok(Directive { revision: Some("one,two"), kind: DirectiveKind::CompileFlags(Vec::new()) })
    );
}

#[test]
fn no_directives() {
    assert_eq!(try_parse("#![crate_type = \"lib\"]\nfn main() {}\n", Scope::Base), default());
}

#[test]
fn compile_flags_directives() {
    assert_eq!(
        try_parse(
            "\n  \t  //@  compile-flags: --crate-type lib\n\
            //@compile-flags:--edition=2021",
            Scope::Base
        ),
        (
            Directives {
                instantiated: InstantiatedDirectives {
                    verbatim_flags: VerbatimFlagsBuf {
                        arguments: vec!["--crate-type", "lib", "--edition=2021"],
                        ..default()
                    },
                    ..default()
                },
                uninstantiated: default(),
            },
            default()
        )
    );
}

#[test]
fn conditional_directives() {
    assert_eq!(
        try_parse(
            "//@ revisions: one two\n\
             //@[one] edition: 2018\n\
             //@ compile-flags: --crate-type=lib\n\
             //@[two] compile-flags: -Zparse-crate-root-only",
            Scope::Base
        ),
        (
            Directives {
                instantiated: InstantiatedDirectives {
                    revisions: ["one", "two"].into(),
                    verbatim_flags: VerbatimFlagsBuf {
                        arguments: vec!["--crate-type=lib"],
                        ..default()
                    },
                    ..default()
                },
                uninstantiated: BTreeMap::from_iter([
                    ("one", vec![DirectiveKind::Edition(Edition::Rust2018),]),
                    ("two", vec![DirectiveKind::CompileFlags(vec!["-Zparse-crate-root-only"])])
                ])
            },
            default()
        )
    );
}
