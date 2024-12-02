use std::borrow::Cow;

pub(crate) type Str = Cow<'static, str>;

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
