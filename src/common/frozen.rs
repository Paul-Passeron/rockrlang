/* Rockr programming language
Copyright (C) 2026  NoRezap

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::{
    fmt,
    hash::Hash,
    mem::{self, MaybeUninit, transmute},
    ops::{Index, IndexMut},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

const BUCKET_SIZE: usize = 32;

pub struct Frozen<T> {
    data: Mutex<Vec<Box<[MaybeUninit<T>; BUCKET_SIZE]>>>,
    next_bucket_idx: AtomicUsize,
}

pub struct FrozenIter<'a, T> {
    frozen: &'a Frozen<T>,
    bucket_idx: usize,
    item_idx: usize,
}

pub struct FrozenIterMut<'a, T> {
    frozen: &'a mut Frozen<T>,
    bucket_idx: usize,
    item_idx: usize,
}

pub struct FrozenIntoIter<T> {
    elems: Vec<T>,
}

impl<T> Frozen<T> {
    pub fn new() -> Self {
        Self { data: Mutex::new(vec![]), next_bucket_idx: AtomicUsize::new(0) }
    }

    pub fn push(&self, item: T) {
        let mut data = self.data.lock().unwrap();
        if data.is_empty() || self.next_bucket_idx.load(Ordering::Relaxed) == BUCKET_SIZE
        {
            self.next_bucket_idx.store(0, Ordering::Relaxed);
            data.push(Box::new([const { MaybeUninit::uninit() }; BUCKET_SIZE]));
        }

        let next_id = self.next_bucket_idx.load(Ordering::Relaxed);
        data.last_mut().unwrap().get_mut(next_id).unwrap().write(item);
        self.next_bucket_idx.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get(&self, idx: usize) -> Option<&T> {
        if self.is_empty() {
            return None;
        }
        let bucket_idx = idx / BUCKET_SIZE;
        let data = self.data.lock().unwrap();
        let next_bucket_idx = self.next_bucket_idx.load(Ordering::Relaxed);
        if bucket_idx >= data.len() {
            return None;
        }
        let idx = idx - bucket_idx * BUCKET_SIZE;
        if bucket_idx == data.len() - 1 && idx >= next_bucket_idx {
            return None;
        }

        let as_ref = unsafe { data[bucket_idx].as_ptr().wrapping_add(idx).as_ref() };
        Some(unsafe { as_ref.unwrap().assume_init_ref() })
    }

    pub fn get_mut(&self, idx: usize) -> Option<&mut T> {
        let bucket_idx = idx / BUCKET_SIZE;
        let data = self.data.lock().unwrap();
        let next_bucket_idx = self.next_bucket_idx.load(Ordering::Relaxed);
        if bucket_idx >= data.len() {
            return None;
        }
        let idx = idx - bucket_idx * BUCKET_SIZE;
        if bucket_idx == data.len() - 1 && idx >= next_bucket_idx {
            return None;
        }
        Some(unsafe {
            data[bucket_idx]
                .as_ptr()
                .wrapping_add(idx)
                .cast_mut()
                .as_mut()
                .unwrap()
                .assume_init_mut()
        })
    }

    pub fn iter(&self) -> FrozenIter<'_, T> {
        FrozenIter { frozen: self, bucket_idx: 0, item_idx: 0 }
    }

    pub fn iter_mut(&mut self) -> FrozenIterMut<'_, T> {
        FrozenIterMut { frozen: self, bucket_idx: 0, item_idx: 0 }
    }

    pub fn is_empty(&self) -> bool {
        let data = self.data.lock().unwrap();
        data.is_empty()
            || (data.len() == 1 && self.next_bucket_idx.load(Ordering::Relaxed) == 0)
    }

    pub fn len(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            let data = self.data.lock().unwrap();
            self.next_bucket_idx.load(Ordering::Relaxed) + (data.len() - 1) * BUCKET_SIZE
        }
    }
}

impl<T> Drop for Frozen<T> {
    fn drop(&mut self) {
        let mut data = self.data.lock().unwrap();
        let next_bucket_idx = self.next_bucket_idx.load(Ordering::Relaxed);
        let last = data.len().wrapping_sub(1);

        for (i, bucket) in data.iter_mut().enumerate() {
            let count = if i == last { next_bucket_idx } else { BUCKET_SIZE };
            unsafe {
                bucket.iter_mut().take(count).for_each(|elem| elem.assume_init_drop());
            }
        }
    }
}

impl<'a, T> Iterator for FrozenIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let res = self.frozen.get(self.item_idx + self.bucket_idx * BUCKET_SIZE);
        self.item_idx += 1;
        if self.item_idx >= BUCKET_SIZE {
            self.item_idx = 0;
            self.bucket_idx += 1;
        }
        res
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.frozen.len();
        (len, Some(len))
    }

    fn count(self) -> usize
    where
        Self: Sized,
    {
        self.frozen.len()
    }
}

impl<'a, T> Iterator for FrozenIterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        let res = unsafe {
            transmute::<Option<&mut T>, Option<&'a mut T>>(
                self.frozen.get_mut(self.item_idx + self.bucket_idx * BUCKET_SIZE),
            )
        };
        self.item_idx += 1;
        if self.item_idx >= BUCKET_SIZE {
            self.item_idx = 0;
            self.bucket_idx += 1;
        }
        res
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.frozen.len();
        (len, Some(len))
    }

    fn count(self) -> usize
    where
        Self: Sized,
    {
        self.frozen.len()
    }
}

impl<T> Iterator for FrozenIntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.elems.pop()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.elems.len();
        (len, Some(len))
    }

    fn count(self) -> usize
    where
        Self: Sized,
    {
        self.elems.len()
    }
}

impl<'a, T> IntoIterator for &'a Frozen<T> {
    type Item = &'a T;

    type IntoIter = FrozenIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Frozen<T> {
    type Item = &'a mut T;

    type IntoIter = FrozenIterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T> IntoIterator for Frozen<T> {
    type Item = T;

    type IntoIter = FrozenIntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        let mut data = self.data.lock().unwrap();
        if data.is_empty() {
            return FrozenIntoIter { elems: vec![] };
        }
        let mut owned_data = vec![];
        mem::swap(data.as_mut(), &mut owned_data);
        let last_item = self.next_bucket_idx.load(Ordering::Relaxed);
        let mut res = vec![];
        let last_box = owned_data.len() - 1;
        for (i, bucket) in owned_data.into_iter().enumerate() {
            let values = *bucket;
            if i == last_box {
                for (idx, item) in values.into_iter().enumerate() {
                    if idx < last_item {
                        unsafe {
                            res.push(item.assume_init());
                        }
                    }
                }
            } else {
                res.extend(values.into_iter().map(|item| unsafe { item.assume_init() }));
            }
        }
        res.reverse();
        self.next_bucket_idx.store(0, Ordering::Relaxed);
        FrozenIntoIter { elems: res }
    }
}

impl<T> Index<usize> for Frozen<T> {
    type Output = T;

    fn index(&self, idx: usize) -> &Self::Output {
        self.get(idx).unwrap()
    }
}

impl<T> IndexMut<usize> for Frozen<T> {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        self.get_mut(idx).unwrap()
    }
}

impl<T: Clone> Clone for Frozen<T> {
    fn clone(&self) -> Self {
        if self.is_empty() {
            return Self::new();
        }
        let data = self.data.lock().unwrap();
        let next_bucket_idx = self.next_bucket_idx.load(Ordering::Relaxed);
        Self {
            data: {
                let last_bucket = data.len() - 1;
                let buckets = data
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        let mut arr = [const { MaybeUninit::uninit() }; BUCKET_SIZE];
                        let max_idx =
                            if i == last_bucket { next_bucket_idx } else { BUCKET_SIZE };
                        for j in 0..max_idx {
                            unsafe {
                                arr[j].write(x[j].assume_init_ref().clone());
                            }
                        }
                        Box::new(arr)
                    })
                    .collect();
                Mutex::new(buckets)
            },
            next_bucket_idx: AtomicUsize::new(next_bucket_idx),
        }
    }
}

impl<T> FromIterator<T> for Frozen<T> {
    fn from_iter<A: IntoIterator<Item = T>>(iter: A) -> Self {
        let this = Self::new();
        iter.into_iter().for_each(|x| this.push(x));
        this
    }
}

impl<T: fmt::Debug> fmt::Debug for Frozen<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Hash> Hash for Frozen<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.iter().for_each(|elem| Hash::hash(elem, state));
    }
}

impl<T> Default for Frozen<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Frozen<T> {
    pub fn is_sorted_by_key<R: Ord>(&self, cmp: impl Fn(&T) -> R) -> bool {
        self.iter().zip(self.iter().skip(1)).all(|(a, b)| cmp(a) < cmp(b))
    }

    pub fn binary_search_by_key<R: Ord>(
        &self,
        value: R,
        cmp: &impl Fn(&T) -> R,
    ) -> Result<usize, usize> {
        debug_assert!(self.is_sorted_by_key(cmp));
        let l = self.len();
        let mut left = 0;
        let mut right = l;
        while left < right {
            let mid = usize::midpoint(left, right);
            if cmp(self.get(mid).unwrap()) < value {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        (left < l && self.get(left).map(cmp) == Some(value)).then_some(left).ok_or(left)
    }
}

impl<T: Ord> Frozen<T> {
    pub fn is_sorted(&self) -> bool {
        self.iter().zip(self.iter().skip(1)).all(|(a, b)| a <= b)
    }

    pub fn sort(&mut self) {
        let this = mem::take(self);
        let mut v = this.into_iter().collect::<Vec<_>>();
        v.sort();
        *self = Self::from_iter(v);
    }

    pub fn into_sorted(self) -> Self {
        let mut this = self;
        this.sort();
        this
    }

    pub fn binary_search(&self, value: &T) -> Result<usize, usize> {
        debug_assert!(self.is_sorted());
        let l = self.len();
        let mut left = 0;
        let mut right = l;
        while left < right {
            let mid = usize::midpoint(left, right);
            if self.get(mid).unwrap() < value {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        (left < l && self.get(left) == Some(value)).then_some(left).ok_or(left)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_get() {
        let frozen = Frozen::new();
        assert_eq!(frozen.get(0), None);
        frozen.push(42);
        assert_eq!(frozen.get(0), Some(&42));
        let ref_ptr: *const i32 = frozen.get(0).unwrap();

        for i in 0..150 {
            frozen.push((i * 31 + 85) % 256);
        }

        let new_ref_ptr: *const i32 = frozen.get(0).unwrap();

        println!("{:p}\n{:p}", ref_ptr, new_ref_ptr);
        assert_eq!(ref_ptr, new_ref_ptr);
    }

    #[test]
    fn test_mut_iter() {
        let mut frozen = Frozen::new();
        for i in 0..100 {
            frozen.push(i);
        }
        for item in frozen.iter_mut() {
            *item += 100;
        }
        for (i, item) in frozen.iter().enumerate() {
            assert_eq!(i + 100, *item);
        }
    }

    #[test]
    fn push_and_index() {
        let f = Frozen::new();
        f.push(10);
        f.push(20);
        f.push(30);
        assert_eq!(f[0], 10);
        assert_eq!(f[1], 20);
        assert_eq!(f[2], 30);
    }

    #[test]
    fn push_takes_shared_ref() {
        let f = Frozen::new();
        let r = &f;
        r.push(1);
        r.push(2);
        assert_eq!(r[0], 1);
        assert_eq!(r[1], 2);
    }

    #[test]
    fn index_mut() {
        let mut f = Frozen::new();
        f.push(0);
        f[0] = 42;
        assert_eq!(f[0], 42);
    }

    #[test]
    #[should_panic]
    fn index_out_of_bounds_panics() {
        let f: Frozen<i32> = Frozen::new();
        let _ = f[0];
    }

    #[test]
    fn len_and_is_empty() {
        let f: Frozen<i32> = Frozen::new();
        assert!(f.is_empty());
        assert_eq!(f.len(), 0);

        f.push(1);
        assert!(!f.is_empty());
        assert_eq!(f.len(), 1);

        f.push(2);
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn iter_yields_shared_refs() {
        let f = Frozen::new();
        f.push(10);
        f.push(20);
        f.push(30);

        let v: Vec<&i32> = f.iter().collect();
        assert_eq!(v, vec![&10, &20, &30]);
    }

    #[test]
    fn iter_mut_allows_mutation() {
        let mut f = Frozen::new();
        f.push(1);
        f.push(2);
        f.push(3);

        for x in f.iter_mut() {
            *x *= 10;
        }

        let v: Vec<&i32> = f.iter().collect();
        assert_eq!(v, vec![&10, &20, &30]);
    }

    #[test]
    fn into_iter_owned() {
        let f = Frozen::new();
        f.push(String::from("a"));
        f.push(String::from("b"));

        let v: Vec<String> = f.into_iter().collect();
        assert_eq!(v, vec!["a", "b"]);
    }

    #[test]
    fn for_loop_shared_ref() {
        let f = Frozen::new();
        f.push(1);
        f.push(2);

        let mut sum = 0;
        for x in &f {
            sum += x;
        }
        assert_eq!(sum, 3);
    }

    #[test]
    fn for_loop_mut_ref() {
        let mut f = Frozen::new();
        f.push(1);
        f.push(2);

        for x in &mut f {
            *x += 10;
        }
        assert_eq!(f[0], 11);
        assert_eq!(f[1], 12);
    }

    #[test]
    fn for_loop_owned() {
        let f = Frozen::new();
        f.push(100);
        f.push(200);

        let mut sum = 0;
        for x in f {
            sum += x;
        }
        assert_eq!(sum, 300);
    }

    #[test]
    fn iter_empty() {
        let f: Frozen<i32> = Frozen::new();
        assert_eq!(f.iter().count(), 0);
    }

    #[test]
    fn iter_size_hint() {
        let f = Frozen::new();
        f.push(1);
        f.push(2);
        f.push(3);

        let iter = f.iter();
        assert_eq!(iter.size_hint(), (3, Some(3)));
    }

    #[test]
    fn references_remain_valid_after_push() {
        let f = Frozen::new();
        f.push(String::from("first"));

        // Grab a pointer to the first element BEFORE more pushes.
        let ptr = &f[0] as *const String;

        // Push more items — may reallocate internal storage.
        for i in 0..100 {
            f.push(format!("item_{i}"));
        }

        // The original pointer must still be valid.
        unsafe {
            assert_eq!(&*ptr, "first");
        }
    }

    #[test]
    fn works_with_strings() {
        let f = Frozen::new();
        f.push(String::from("hello"));
        f.push(String::from("world"));
        assert_eq!(&f[0], "hello");
        assert_eq!(&f[1], "world");
    }

    #[test]
    fn works_with_vec() {
        let f: Frozen<Vec<u8>> = Frozen::new();
        f.push(vec![1, 2, 3]);
        f.push(vec![4, 5]);
        assert_eq!(f[0], vec![1, 2, 3]);
    }

    #[test]
    fn drops_elements() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct Noisy;
        impl Drop for Noisy {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }

        DROP_COUNT.store(0, Ordering::SeqCst);
        {
            let f = Frozen::new();
            f.push(Noisy);
            f.push(Noisy);
            f.push(Noisy);
        }
        assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn push_many() {
        let f = Frozen::new();
        for i in 0..10_000 {
            f.push(i);
        }
        assert_eq!(f.len(), 10_000);
        assert_eq!(f[0], 0);
        assert_eq!(f[9_999], 9_999);

        let sum: i64 = f.iter().map(|&x| x as i64).sum();
        for elem in f.iter() {
            println!("{elem:?}")
        }

        assert_eq!(sum, (10_000i64 * 9_999) / 2);
    }

    #[test]
    fn collect_from_iterator() {
        let f: Frozen<i32> = (0..5).collect();
        assert_eq!(f.len(), 5);
        for i in 0..5 {
            assert_eq!(f[i], i as i32);
        }
    }

    #[test]
    fn debug_fmt() {
        let f = Frozen::new();
        f.push(1);
        f.push(2);
        // Just make sure it doesn't panic; exact format is up to you.
        let s = format!("{f:?}");
        assert!(!s.is_empty());
    }
}
