use crate::utility::default;
use std::cell::{Ref, RefCell};

pub(crate) struct MonotonicVec<T> {
    items: RefCell<Vec<T>>,
}

impl<T> MonotonicVec<T> {
    pub(crate) fn push(&self, item: T) {
        self.items.borrow_mut().push(item);
    }

    pub(crate) fn get(&self, index: usize) -> Option<Ref<'_, T>> {
        Ref::filter_map(self.items.borrow(), |items| items.get(index)).ok()
    }

    pub(crate) fn last(&self) -> Option<Ref<'_, T>> {
        Ref::filter_map(self.items.borrow(), |items| items.last()).ok()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Ref<'_, T>> {
        (0..).map(|index| self.get(index)).take_while(|item| item.is_some()).flatten()
    }
}

impl<T> Default for MonotonicVec<T> {
    fn default() -> Self {
        Self { items: default() }
    }
}
