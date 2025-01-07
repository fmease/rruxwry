use crate::utility::default;
use std::cell::{Ref, RefCell};

pub(crate) struct MonotonicVec<T> {
    items: RefCell<Vec<T>>,
}

impl<T> MonotonicVec<T> {
    pub(crate) fn push(&self, item: T) -> usize {
        let mut items = self.items.borrow_mut();
        let index = items.len();
        items.push(item);
        index
    }

    pub(crate) fn get(&self, index: usize) -> Option<Ref<'_, T>> {
        Ref::filter_map(self.items.borrow(), |items| items.get(index)).ok()
    }

    pub(crate) fn last(&self) -> Option<Ref<'_, T>> {
        Ref::filter_map(self.items.borrow(), |items| items.last()).ok()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Ref<'_, T>> {
        (0..).filter_map(|index| self.get(index)).fuse()
    }
}

impl<T> Default for MonotonicVec<T> {
    fn default() -> Self {
        Self { items: default() }
    }
}
