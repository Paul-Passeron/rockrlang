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

    println!("{ref_ptr:p}\n{new_ref_ptr:p}");
    assert_eq!(ref_ptr, new_ref_ptr);
}

#[test]
fn test_mut_iter() {
    let mut frozen = Frozen::new();
    for i in 0..100 {
        frozen.push(i);
    }
    for item in &mut frozen {
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
#[should_panic = "called `Option::unwrap()` on a `None` value"]
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

    for x in &mut f {
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
    let ptr = &raw const f[0];

    // Push more items — may reallocate internal storage.
    for i in 0..100 {
        f.push(format!("item_{i}"));
    }
    // SAFETY:
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

    let sum: i64 = f.iter().map(|&x| i64::from(x)).sum();
    for elem in &f {
        println!("{elem:?}");
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
    assert_ne!(s, "");
}
