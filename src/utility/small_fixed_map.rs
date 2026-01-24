use std::marker::PhantomData;

pub(crate) struct SmallFixedMap<K: SmallFixedKey, V> {
    data: [Option<V>; K::LEN],
    _marker: PhantomData<fn(&K)>,
}

impl<K: SmallFixedKey, V> SmallFixedMap<K, V> {
    pub(crate) fn get(&self, key: K) -> Option<&V> {
        self.data[key.index()].as_ref()
    }

    pub(crate) fn get_or_insert(&mut self, key: K, value: V) -> &mut V {
        self.data[key.index()].get_or_insert(value)
    }
}

impl<K: SmallFixedKey, V> Default for SmallFixedMap<K, V> {
    fn default() -> Self {
        Self { data: [const { None }; _], _marker: PhantomData }
    }
}

pub(crate) trait SmallFixedKey: Copy {
    #[expect(dead_code)] // FIXME: rustc false positive
    #[type_const]
    const LEN: usize;
    fn index(self) -> usize;
}

pub(crate) macro SmallFixedKey {
    derive() ($vis:vis enum $name:ident { $($variant:ident),* $(,)? }) => {
        impl SmallFixedKey for $name {
            #[type_const]
            const LEN: usize = ${count($variant)};

            fn index(self) -> usize {
                self as _
            }
        }
    }
}
