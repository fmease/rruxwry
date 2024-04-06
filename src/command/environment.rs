use crate::utility::Tag;
use rustc_hash::FxHashMap;
use std::{
    ffi::{OsStr, OsString},
    sync::LazyLock,
};

type Environment = FxHashMap<OsString, OsString>;

pub(crate) fn rustc_flags<'a>() -> Option<&'a [String]> {
    static RUSTFLAGS: LazyLock<Option<Vec<String>>> = LazyLock::new(|| {
        parse_flags(
            OsStr::new("RUSTFLAGS"),
            &[
                OsStr::new("RUST_FLAGS"),
                OsStr::new("RUSTCFLAGS"),
                OsStr::new("RUSTC_FLAGS"),
            ],
            &ENVIRONMENT,
        )
    });

    RUSTFLAGS.as_deref()
}

pub(crate) fn rustdoc_flags<'a>() -> Option<&'a [String]> {
    static RUSTDOCFLAGS: LazyLock<Option<Vec<String>>> = LazyLock::new(|| {
        parse_flags(
            OsStr::new("RUSTDOCFLAGS"),
            &[OsStr::new("RUSTDOC_FLAGS")],
            &ENVIRONMENT,
        )
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
            eprintln!(
                "{}rrustdoc does not read the `{}` environment variable; \
                 you might have meant `{}`",
                Tag::Warning,
                confusable.display(),
                key.display(),
            );
        }
    }

    let flags = environment.get(key)?;

    let Some(flags) = flags.to_str() else {
        eprintln!(
            "{}the environment variable `{}` does not contain valid UTF-8; \
             ignoring all potential flags",
            Tag::Warning,
            key.display(),
        );

        return None;
    };

    let flags = shlex::split(&flags);

    if flags.is_none() {
        eprintln!(
            "{}the environment variable `{}` is not well-formed; \
             ignoring all potential flags",
            Tag::Warning,
            key.display(),
        );
    }

    flags
}
