use super::{Directive, DirectiveKind, DirectiveParser, Error, ErrorKind};

fn parse_directive(source: &str) -> Result<Directive<'_>, Error<'_>> {
    DirectiveParser::new(source).execute()
}

#[test]
fn unknown_compiletest_directive() {
    assert_eq!(
        parse_directive("only-unix"),
        Err(Error {
            kind: ErrorKind::UnknownDirective("only-unix"),
            context: None,
        })
    )
}

#[test]
fn empty_compiletest_directive() {
    assert_eq!(
        parse_directive(""),
        Err(Error {
            kind: ErrorKind::UnexpectedEndOfInput,
            context: None,
        }),
    );
}

#[test]
fn whitespace_only_compiletest_directive() {
    assert_eq!(
        parse_directive("  \t   "),
        Err(Error {
            kind: ErrorKind::UnexpectedEndOfInput,
            context: None
        })
    )
}

#[test]
fn build_aux_docs_directive() {
    assert_eq!(
        parse_directive("build-aux-docs"),
        Ok(Directive {
            revision: None,
            kind: DirectiveKind::BuildAuxDocs,
        })
    );
}

#[test]
fn padded_build_aux_docs_directive() {
    assert_eq!(
        parse_directive(" \t  build-aux-docs "),
        Ok(Directive {
            revision: None,
            kind: DirectiveKind::BuildAuxDocs,
        })
    );
}

#[test]
fn empty_revisions_directive() {
    todo!() // FIXME: how does `compiletest` handle this?
}

#[test]
fn revisions_directive() {
    assert_eq!(
        parse_directive("revisions: one two  three"),
        Ok(Directive {
            revision: None,
            kind: DirectiveKind::Revisions(vec!["one", "two", "three"])
        })
    );
}
