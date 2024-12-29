//! Dealing with environment variables.

use crate::command::emit;
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    sync::LazyLock,
};

type Environment = HashMap<OsString, OsString>;

pub(super) fn rustc_flags<'a>() -> Option<&'a [String]> {
    static RUSTFLAGS: LazyLock<Option<Vec<String>>> = LazyLock::new(|| {
        parse_flags(
            OsStr::new("RUSTFLAGS"),
            &[OsStr::new("RUST_FLAGS"), OsStr::new("RUSTCFLAGS"), OsStr::new("RUSTC_FLAGS")],
            &ENVIRONMENT,
        )
    });

    RUSTFLAGS.as_deref()
}

pub(super) fn rustdoc_flags<'a>() -> Option<&'a [String]> {
    static RUSTDOCFLAGS: LazyLock<Option<Vec<String>>> = LazyLock::new(|| {
        parse_flags(OsStr::new("RUSTDOCFLAGS"), &[OsStr::new("RUSTDOC_FLAGS")], &ENVIRONMENT)
    });

    RUSTDOCFLAGS.as_deref()
}

static ENVIRONMENT: LazyLock<Environment> = LazyLock::new(|| std::env::vars_os().collect());

fn parse_flags(
    key: &OsStr,
    confusables: &[&OsStr],
    environment: &Environment,
) -> Option<Vec<String>> {
    for &confusable in confusables {
        if environment.contains_key(confusable) {
            warn_env_contains_confusable_var(confusable, key);
        }
    }

    let flags = environment.get(key)?;

    let Some(flags) = flags.to_str() else {
        warn_malformed_env_var(key, "its content is not valid UTF-8");

        return None;
    };

    let flags = shlex::split(flags);

    if flags.is_none() {
        warn_malformed_env_var(key, "its content is not properly escaped");
    }

    flags
}

fn warn_env_contains_confusable_var(confusable: &OsStr, suggestion: &OsStr) {
    emit!(
        Warning("rruxwry does not read the environment variable `{}`", confusable.display())
            .note("you might have meant `{}`", suggestion.display())
    );
}

fn warn_malformed_env_var(key: &OsStr, note: &'static str) {
    emit!(
        Warning("the environment variable `{}` is malformed", key.display())
            .note("{note}")
            .note("ignoring all flags potentially contained within it")
    );
}
