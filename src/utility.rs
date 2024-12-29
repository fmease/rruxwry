pub(crate) mod paint;
pub(crate) mod parse;

pub(crate) fn default<T: Default>() -> T {
    T::default()
}

pub(crate) type SmallVec<T, const N: usize> = smallvec::SmallVec<[T; N]>;

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
