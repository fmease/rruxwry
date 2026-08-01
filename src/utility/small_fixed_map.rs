use core::direct_const_arg as lift;
use std::marker::PhantomData;

pub(crate) struct SmallFixedMap<K: SmallKey, V> {
    data: [Option<V>; lift!(K::LEN)],
    _marker: PhantomData<fn(&K)>,
}

impl<K: SmallKey, V> SmallFixedMap<K, V> {
    pub(crate) fn get(&self, key: K) -> Option<&V> {
        self.data[key.index()].as_ref()
    }

    pub(crate) fn get_or_insert(&mut self, key: K, value: V) -> &mut V {
        self.data[key.index()].get_or_insert(value)
    }
}

impl<K: SmallKey, V> Default for SmallFixedMap<K, V> {
    fn default() -> Self {
        Self { data: [const { None }; _], _marker: PhantomData }
    }
}

pub(crate) trait SmallKey: Copy {
    #[expect(dead_code)] // FIXME: rustc false positive
    type const LEN: usize;
    fn index(self) -> usize;
}

pub(crate) macro SmallKey {
    derive() ($vis:vis enum $name:ident { $($variant:ident),* $(,)? }) => {
        impl SmallKey for $name {
            type const LEN: usize = ${count($variant)};

            fn index(self) -> usize {
                self as _
            }
        }
    }
}
