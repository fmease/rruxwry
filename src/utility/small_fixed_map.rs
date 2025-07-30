use std::marker::PhantomData;

// FIXME: Use mGCA here instead of GCE once it no longer ICEs for the rest of the crate
//        (this data structure typecks in isolation as tested on the playground).
pub(crate) struct SmallFixedMap<K: SmallFixedKey, V>
where
    WellFormedKey<K>:,
{
    data: [Option<V>; K::LEN],
    _marker: PhantomData<fn(&K)>,
}

impl<K: SmallFixedKey, V> SmallFixedMap<K, V>
where
    WellFormedKey<K>:,
{
    pub(crate) fn get(&self, key: K) -> Option<&V> {
        self.data[key.index()].as_ref()
    }

    pub(crate) fn get_or_insert(&mut self, key: K, value: V) -> &mut V {
        self.data[key.index()].get_or_insert(value)
    }
}

impl<K: SmallFixedKey, V> Default for SmallFixedMap<K, V>
where
    WellFormedKey<K>:,
{
    fn default() -> Self {
        // FIXME: Use `const { None }` instead of dummy const item once we've switched to mGCA.
        //        The workaround is necessary for GCE since it deems inline consts "overly complex".
        const NONE<T>: Option<T> = None;
        Self { data: [NONE; _], _marker: PhantomData }
    }
}

pub(crate) trait SmallFixedKey: Copy {
    const LEN: usize;
    fn index(self) -> usize;
}

// FIXME: Remove once we're based on mGCA instead of GCE.
pub(crate) type WellFormedKey<K: SmallFixedKey> = [(); K::LEN];
