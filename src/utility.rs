use std::{ascii::Char, ffi::OsStr};

pub(crate) mod monotonic;
pub(crate) mod paint;
pub(crate) mod small_fixed_map;

use crate::context::Context;
pub(crate) use rustc_hash::FxHashMap as HashMap;

pub(crate) fn default<T: Default>() -> T {
    T::default()
}

pub(crate) macro parse($( $( $key:literal )|+ => $value:expr ),+ $(,)?) {
    |source| Ok(match source {
        $( $( $key )|+ => $value, )+
        _ => return Err([$( $( $key ),+ ),+].into_iter()),
    })
}

pub(crate) trait ListingExt {
    fn list(self, conjunction: Conjunction) -> String;
}

impl<I: Iterator<Item: Clone + std::fmt::Display>> ListingExt for I {
    fn list(self, conjunction: Conjunction) -> String {
        let mut items = self.peekable();
        let mut first = true;
        let mut result = String::new();

        while let Some(item) = items.next() {
            if !first {
                if items.peek().is_some() {
                    result += ", ";
                } else {
                    result += " ";
                    result += conjunction.to_str();
                    result += " ";
                }
            }

            result += &item.to_string();
            first = false;
        }

        result
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Conjunction {
    And,
    Or,
}

impl Conjunction {
    const fn to_str(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
        }
    }
}

pub(crate) trait OsStrExt {
    fn rsplit_once(&self, pat: Char) -> Option<(&OsStr, &OsStr)>;
}

impl OsStrExt for OsStr {
    fn rsplit_once(&self, pat: Char) -> Option<(&OsStr, &OsStr)> {
        let (a, b) = self.as_encoded_bytes().rsplit_once(|&byte| byte == pat as _)?;
        // SAFETY: Each substring was separated by a 7-bit ASCII char (length-one substring).
        let a = unsafe { OsStr::from_encoded_bytes_unchecked(a) };
        let b = unsafe { OsStr::from_encoded_bytes_unchecked(b) };
        Some((a, b))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    pub(crate) fn colorize(self, cx: Context<'_>) -> bool {
        crate::context::invoke!(cx.colorize(self))
    }
}

fn colorize(stream: Stream, _: Context<'_>) -> bool {
    // FIXME: Awkward workaround.
    let choice = match stream {
        Stream::Stdout => anstream::AutoStream::choice(&std::io::stdout()),
        Stream::Stderr => anstream::AutoStream::choice(&std::io::stderr()),
    };
    choice != anstream::ColorChoice::Never
}
