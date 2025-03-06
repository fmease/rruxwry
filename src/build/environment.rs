//! Dealing with environment variables.

// We cache everything in statics because `command` may call these functions over and over if there are
// multiple source files. Probably not worth it or possibly worse than not caching at all.

use crate::{
    data::Identity,
    diagnostic::{fmt, warn},
};
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    sync::LazyLock,
};

type Environment = HashMap<OsString, OsString>;

pub(super) fn rustc_options<'a>() -> Option<&'a [String]> {
    static OPTS: LazyLock<Option<Vec<String>>> = LazyLock::new(|| {
        parse_options(
            OsStr::new("RUSTFLAGS"),
            &[OsStr::new("RUST_FLAGS"), OsStr::new("RUSTCFLAGS"), OsStr::new("RUSTC_FLAGS")],
            &ENVIRONMENT,
        )
    });

    OPTS.as_deref()
}

pub(super) fn rustdoc_options<'a>() -> Option<&'a [String]> {
    static OPTS: LazyLock<Option<Vec<String>>> = LazyLock::new(|| {
        parse_options(OsStr::new("RUSTDOCFLAGS"), &[OsStr::new("RUSTDOC_FLAGS")], &ENVIRONMENT)
    });

    OPTS.as_deref()
}

// FIXME: Also support `RUSTC_BOOTSTRAP=$crate_name`.
// FIXME: Cache this, too?
pub(super) fn identity_uncached() -> Option<Identity> {
    Some(match ENVIRONMENT.get(OsStr::new("RUSTC_BOOTSTRAP"))?.as_encoded_bytes() {
        b"1" => Identity::Nightly,
        b"-1" => Identity::Stable,
        _ => Identity::True,
    })
}

static ENVIRONMENT: LazyLock<Environment> = LazyLock::new(|| std::env::vars_os().collect());

fn parse_options(
    key: &OsStr,
    confusables: &[&OsStr],
    environment: &Environment,
) -> Option<Vec<String>> {
    for &confusable in confusables {
        if environment.contains_key(confusable) {
            warn_env_contains_confusable_var(confusable, key);
        }
    }

    let opts = environment.get(key)?;

    let Some(opts) = opts.to_str() else {
        warn_malformed_env_var(key, "its content is not valid UTF-8");

        return None;
    };

    let opts = shlex::split(opts);

    if opts.is_none() {
        warn_malformed_env_var(key, "its content is not properly escaped");
    }

    opts
}

fn warn_env_contains_confusable_var(confusable: &OsStr, suggestion: &OsStr) {
    // FIXME: We now say "warning[rruxwry] rruxwry ..." which is meh. Rephrase!
    warn(fmt!("rruxwry does not read the environment variable `{}`", confusable.display()))
        .note(fmt!("you might have meant `{}`", suggestion.display()))
        .finish();
}

fn warn_malformed_env_var(key: &OsStr, note: &'static str) {
    // FIXME: Make this a (hard/fatal) error.
    warn(fmt!("the environment variable `{}` is malformed", key.display()))
        .note(fmt!("{note}"))
        .note(fmt!("ignoring all flags potentially contained within it"))
        .finish();
}
