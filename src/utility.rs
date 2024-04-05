use owo_colors::{AnsiColors, OwoColorize};
use std::fmt;

#[derive(Clone, Copy)]
pub(crate) enum Tag {
    Note,
    Warning,
    #[allow(dead_code)] // FIXME
    Error,
}

impl Tag {
    const fn name(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    const fn color(self) -> AnsiColors {
        match self {
            Self::Note => AnsiColors::Cyan,
            Self::Warning => AnsiColors::Yellow,
            Self::Error => AnsiColors::Red,
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.name().color(Tag::color(*self)))
    }
}

pub(crate) fn default<T: Default>() -> T {
    T::default()
}

pub(crate) type SmallVec<T, const N: usize> = smallvec::SmallVec<[T; N]>;

pub(crate) trait Captures<'a> {}

impl<'a, T: ?Sized> Captures<'a> for T {}
