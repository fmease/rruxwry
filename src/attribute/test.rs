use super::Attributes;
use crate::data::{CrateName, CrateType, Edition};

// FIXME: Test that we detect `#![crate_name = "0"]` (leading digits) to match rustc's behavior.

fn parse(source: &str) -> Attributes<'_> {
    Attributes::parse(source, &[], Edition::Edition2015, false)
}

#[test]
fn crate_name() {
    assert_eq!(parse(r#"#![crate_name = "name"]"#), Attributes {
        crate_name: Some(CrateName::new_unchecked("name")),
        crate_type: None
    });
}

#[test]
fn crate_type_crate_name() {
    assert_eq!(parse(r#"#![crate_type = "proc-macro"]#![crate_name = "alias"]"#), Attributes {
        crate_name: Some(CrateName::new_unchecked("alias")),
        crate_type: Some(CrateType::ProcMacro),
    });
}

#[test]
fn crate_name_spaced() {
    assert_eq!(parse(r#" # ! [ crate_name = "name" ] "#), Attributes {
        crate_name: Some(CrateName::new_unchecked("name")),
        crate_type: None,
    });
}

#[test]
fn crate_name_interleaved_trivia() {
    assert_eq!(
        parse(
            r#"
#/* */!/* */[
    // key
    crate_name
    // separator
    =
    // value
    /*-->*/"alias"/*<--*/
]
"#
        ),
        Attributes { crate_name: Some(CrateName::new_unchecked("alias")), crate_type: None }
    );
}

#[test]
fn crate_name_leading_inner_attributes() {
    assert_eq!(
        parse(
            r#"
//! Module-level documentation.
#![feature(rustc_attrs)]
#![cfg_attr(not(FALSE), doc = "\n")]
#![crate_name = "name"]
"#,
        ),
        Attributes { crate_name: Some(CrateName::new_unchecked("name")), crate_type: None }
    );
}

#[test]
fn crate_name_not_at_beginning_leading_item() {
    assert_eq!(
        parse(
            r#"
fn main() {}
#![crate_name = "name"]
    "#
        ),
        Attributes::default()
    );
}

#[test]
fn crate_name_not_at_beginning_leading_outer_attribute() {
    assert_eq!(
        parse(
            r#"
#[allow(unused)] // <-- notice the lack of `!` here
#![crate_name = "name"]
"#
        ),
        Attributes::default()
    );
}

#[test]
fn crate_name_red_herrings() {
    assert_eq!(
        parse(
            r#"
#![::crate_name = "no"]
#![crate::crate_name = "no"]
#![crate_name = "yes"]
"#
        ),
        Attributes { crate_name: Some(CrateName::new_unchecked("yes")), crate_type: None }
    );
}

#[test]
fn crate_name_semantically_malformed_leading_attributes() {
    assert_eq!(
        parse(
            r#"
#![::crate_type = "lib"]
#![crate::crate_type = "lib"]
#![crate_type("lib")]
#![crate_type["lib"]]
#![crate_type{"lib"}]
#![crate_name = "krate"]
"#
        ),
        Attributes { crate_name: Some(CrateName::new_unchecked("krate")), crate_type: None }
    );
}

#[test]
fn crate_type_invalid() {
    assert_eq!(parse(r#"#![crate_type = "garbage"]"#), Attributes::default())
}

#[test]
fn crate_name_multiple() {
    // Yes, this matches the behavior of rustc (with an without `--print=crate-name`).
    assert_eq!(parse(r#"#![crate_name = "first"]#![crate_name = "second"]"#), Attributes {
        crate_name: Some(CrateName::new_unchecked("first")),
        crate_type: None,
    });
}

#[test]
fn crate_name_garbage() {
    assert_eq!(parse(r#"#![crate_name = "?"]"#), Attributes::default());
}

#[test]
fn crate_name_leading_digit() {
    // Yes, this matches the behavior of rustc (with an without `--print=crate-name`).
    assert_eq!(parse(r#"#![crate_name = "0"]"#), Attributes {
        crate_name: Some(CrateName::new_unchecked("0")),
        crate_type: None,
    });
}
