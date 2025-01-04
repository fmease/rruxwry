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
    assert_eq!(Directive::parse("", Scope::Base), Err(Error::UnexpectedEndOfInput));
}

#[test]
fn blank_directive() {
    assert_eq!(Directive::parse("  \t   ", Scope::Base), Err(Error::UnexpectedEndOfInput))
}

#[test]
fn unknown_directive() {
    assert_eq!(
        Directive::parse("  undefined ", Scope::Base),
        Err(Error::UnknownDirective("undefined"))
    )
}

#[test]
fn unavailable_directive_base() {
    assert_eq!(Directive::parse("!has", Scope::Base), Err(Error::UnavailableDirective("!has")));
}

#[test]
fn unavailable_directive_htmldocck() {
    assert_eq!(Directive::parse("is", Scope::HtmlDocCk), Err(Error::UnavailableDirective("is")));
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
        Err(Error::UnexpectedToken { found: '2', expected: ':' })
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
    assert_eq!(
        Directive::parse("revisions: repeat repeat", Scope::Base),
        Err(Error::DuplicateRevisions)
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
    assert_eq!(Directive::parse("[predicate]", Scope::Base), Err(Error::UnexpectedEndOfInput));
}

#[test]
fn unknown_conditional_directive() {
    assert_eq!(
        Directive::parse("[whatever] unknown", Scope::Base),
        Err(Error::UnknownDirective("unknown"))
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

// FIXME: This should probably be a `directive::try_parse` test
#[test]
fn conditional_directives_directive() {
    // FIXME: This should also emit a warning.
    assert_eq!(
        Directive::parse("[recur] revisions: recur", Scope::Base),
        Ok(Directive { revision: Some("recur"), kind: DirectiveKind::Revisions(vec!["recur"]) })
    );
}

#[test]
fn no_directives() {
    let mut errors = ErrorBuffer::default();
    let directives = parse("#![crate_type = \"lib\"]\nfn main() {}\n", Scope::Base, &mut errors);
    assert_eq!((directives, errors), default());
}

#[test]
fn compile_flags_directives() {
    let mut errors = ErrorBuffer::default();
    let directives = parse(
        "\n  \t  //@  compile-flags: --crate-type lib\n\
        //@compile-flags:--edition=2021",
        Scope::Base,
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
    let mut errors = ErrorBuffer::default();
    let directives = parse(
        "//@ revisions: one two\n\
         //@[one] edition: 2018\n\
         //@ compile-flags: --crate-type=lib\n\
         //@[two] compile-flags: -Zparse-crate-root-only",
        Scope::Base,
        &mut errors,
    );
    assert_eq!(directives, Directives {
        instantiated: InstantiatedDirectives {
            revisions: ["one", "two"].into(),
            verbatim_flags: VerbatimFlagsBuf { arguments: vec!["--crate-type=lib"], ..default() },
            ..default()
        },
        uninstantiated: BTreeMap::from_iter([
            ("one", vec![DirectiveKind::Edition(Edition::Rust2018),]),
            ("two", vec![DirectiveKind::CompileFlags(vec!["-Zparse-crate-root-only"])])
        ])
    });
    assert_eq!(errors, default());
}

#[test]
fn conditional_directives_revision_declared_after_use() {
    let mut errors = ErrorBuffer::default();
    let directives = parse(
        "//@[next] compile-flags: -Znext-solver\n\
         //@ revisions: classic next",
        Scope::Base,
        &mut errors,
    );
    assert_eq!(directives, Directives {
        instantiated: InstantiatedDirectives { revisions: ["classic", "next"].into(), ..default() },
        uninstantiated: [("next", vec![DirectiveKind::CompileFlags(vec!["-Znext-solver"])])].into(),
    });
    assert_eq!(errors, default());
}

#[test]
fn conditional_directives_undeclared_revisions() {
    let mut errors = ErrorBuffer::default();
    let directives = parse(
        "//@[block] compile-flags: --crate-type lib\n\
         //@[wall] edition: 2021",
        Scope::Base,
        &mut errors,
    );
    assert_eq!(directives, Directives {
        instantiated: default(),
        uninstantiated: [
            ("block", vec![DirectiveKind::CompileFlags(vec!["--crate-type", "lib"]),]),
            ("wall", vec![DirectiveKind::Edition(Edition::Rust2021)]),
        ]
        .into()
    });
    assert_eq!(errors, ErrorBuffer {
        errors: vec![
            Error::UndeclaredRevision { revision: "block", available: default() },
            Error::UndeclaredRevision { revision: "wall", available: default() },
        ],
        ..default()
    });
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
    let mut errors = ErrorBuffer::default();
    let directives = parse("//@ revisions: first second", Scope::Base, &mut errors);
    assert_eq!(errors, default());
    assert_eq!(
        directives.instantiate(None),
        Err(InstantiationError::MissingActiveRevision { available: ["first", "second"].into() })
    );
}
