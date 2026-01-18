use std::{ascii::Char, ffi::OsStr};

pub(crate) mod monotonic;
pub(crate) mod paint;
pub(crate) mod small_fixed_map;

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
    fn strip_prefix(&self, pat: Char) -> Option<&OsStr>;
    fn rsplit_once(&self, pat: Char) -> Option<(&OsStr, &OsStr)>;
}

impl OsStrExt for OsStr {
    fn strip_prefix(&self, pat: Char) -> Option<&OsStr> {
        let s = self.as_encoded_bytes().strip_prefix(std::slice::from_ref(&pat.to_u8()))?;
        // SAFETY: We're allowed to split the bytes coming from `as_encoded_bytes` immediately
        //         after a valid non-empty UTF-8 substring which the pattern trivially satisfies
        //         being a length-one 7-bit ASCII char subslice.
        let s = unsafe { OsStr::from_encoded_bytes_unchecked(s) };
        Some(s)
    }

    fn rsplit_once(&self, pat: Char) -> Option<(&OsStr, &OsStr)> {
        let (s, t) = self.as_encoded_bytes().rsplit_once(|&byte| byte == pat as _)?;
        // FIXME: Further elaborate this explanation:
        // SAFETY: Each substring was separated by a 7-bit ASCII char (length-one substring).
        let s = unsafe { OsStr::from_encoded_bytes_unchecked(s) };
        let t = unsafe { OsStr::from_encoded_bytes_unchecked(t) };
        Some((s, t))
    }
}
