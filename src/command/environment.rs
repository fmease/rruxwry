//! Dealing with environment variables.

use rustc_hash::FxHashMap;
use std::{
    ffi::{OsStr, OsString},
    sync::LazyLock,
};

type Environment = FxHashMap<OsString, OsString>;

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
            warning::environment_contains_confusable_variable(confusable, key).emit();
        }
    }

    let flags = environment.get(key)?;

    let Some(flags) = flags.to_str() else {
        warning::malformed_environment_variable(key, "its content is not valid UTF-8").emit();

        return None;
    };

    let flags = shlex::split(flags);

    if flags.is_none() {
        warning::malformed_environment_variable(key, "its content is not properly escaped").emit();
    }

    flags
}

mod warning {
    use crate::diagnostic::{Diagnostic, warning};
    use std::ffi::OsStr;

    pub(super) fn environment_contains_confusable_variable(
        confusable: &OsStr,
        suggestion: &OsStr,
    ) -> Diagnostic {
        warning(format!(
            "rruxwry does not read the environment variable `{}`",
            confusable.display()
        ))
        .note(format!("you might have meant `{}`", suggestion.display()))
    }

    pub(super) fn malformed_environment_variable(key: &OsStr, note: &'static str) -> Diagnostic {
        warning(format!("the environment variable `{}` is malformed", key.display()))
            .note(note)
            .note("ignoring all flags potentially contained within it")
    }
}
