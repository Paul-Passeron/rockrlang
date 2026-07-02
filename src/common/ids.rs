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

use std::{cell::RefCell, marker::PhantomData};

pub struct IdGen {
    next: RefCell<usize>,
}

impl IdGen {
    pub fn new() -> Self {
        Self { next: RefCell::new(0) }
    }

    pub fn fresh(&self) -> usize {
        let res = *self.next.borrow();
        *self.next.borrow_mut() += 1;
        res
    }
}

impl Default for IdGen {
    fn default() -> Self {
        Self::new()
    }
}

pub struct IdWrapper<T: From<usize>> {
    inner: IdGen,
    _brand: PhantomData<T>,
}

impl<T: From<usize>> IdWrapper<T> {
    pub fn new() -> Self {
        Self { inner: IdGen::new(), _brand: Default::default() }
    }

    pub fn fresh(&self) -> T {
        self.inner.fresh().into()
    }
}

impl<T: From<usize>> Default for IdWrapper<T> {
    fn default() -> Self {
        Self::new()
    }
}
