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

use crate::{Constraint, ConstraintSolveResult, ConstraintSolver};
use std::hash::Hash;

pub trait SolverHost: Sized {
    type Kind: Clone + Eq + Hash;
    type Listener: Copy + Ord + Hash;
    type Metadata;
    type Err;

    fn solver(&mut self) -> &mut ConstraintSolver<Self>;
    fn solve(&mut self, c: &Constraint<Self>) -> ConstraintSolveResult<Self::Err>;
    fn apply_default(&mut self, c: &Constraint<Self>)
    -> ConstraintSolveResult<Self::Err>;
    fn has_default(kind: &Self::Kind) -> bool;
    fn listeners(
        &mut self,
        kind: &Self::Kind,
    ) -> impl IntoIterator<Item = Self::Listener>;
}
