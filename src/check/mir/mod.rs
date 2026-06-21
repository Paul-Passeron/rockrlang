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

use crate::{Db, check::mir::{loans::check_loans, use_after_move::check_use_after_move}, mir::MIR};

pub mod use_after_move;
pub mod loans;

pub fn check_mir(db: &dyn Db, mir: &MIR) {
    check_use_after_move(db, mir);
    check_loans(db, mir);
}
