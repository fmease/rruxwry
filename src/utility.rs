use std::borrow::Cow;

pub(crate) type Str = Cow<'static, str>;

pub(crate) fn default<T: Default>() -> T {
    T::default()
}

pub(crate) type SmallVec<T, const N: usize> = smallvec::SmallVec<[T; N]>;
