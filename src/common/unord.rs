#![allow(dead_code)]
use std::hash::Hash;

use salsa::Update;

/// Unordered collection of items for easy interning of set-like objects in salsa

#[derive(Clone)]
pub struct Set<T: Eq + Ord> {
    items: Vec<T>,
}

impl<T> Set<T>
where
    T: Eq + Ord,
{
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn insert(&mut self, item: T) {
        if !self.items.contains(&item) {
            self.items.push(item);
            self.items.sort();
        }
    }

    pub fn contains(&self, item: &T) -> bool {
        self.items.binary_search(item).is_ok()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

impl<T> Default for Set<T>
where
    T: Eq + Ord,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> IntoIterator for Set<T>
where
    T: Eq + Ord,
{
    type Item = T;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<T> AsRef<[T]> for Set<T>
where
    T: Eq + Ord,
{
    fn as_ref(&self) -> &[T] {
        &self.items
    }
}

impl<T> Hash for Set<T>
where
    T: Eq + Ord + Hash,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.items.hash(state);
    }
}

impl<T> PartialEq for Set<T>
where
    T: Eq + Ord,
{
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl<T> Eq for Set<T> where T: Eq + Ord {}

impl<T> FromIterator<T> for Set<T>
where
    T: Eq + Ord,
{
    fn from_iter<A: IntoIterator<Item = T>>(iter: A) -> Self {
        let mut v = iter.into_iter().collect::<Vec<_>>();
        v.sort();
        Self { items: v }
    }
}

impl<T> From<Vec<T>> for Set<T>
where
    T: Eq + Ord,
{
    fn from(value: Vec<T>) -> Self {
        let mut v = value;
        v.sort();
        Self { items: v }
    }
}

unsafe impl<T> Update for Set<T>
where
    T: Eq + Ord + Update,
{
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        unsafe {
            old_pointer.as_mut().map_or(true, |val| {
                Vec::<T>::maybe_update(&mut val.items, new_value.items)
            })
        }
    }
}
