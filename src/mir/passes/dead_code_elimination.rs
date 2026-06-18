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

use std::collections::HashMap;

use crate::{
    Db,
    mir::{MIR, MIRBlockID, builder::MIRBuilder, passes::MIRPass},
};

pub struct DeadCodeElimination;

impl MIRPass for DeadCodeElimination {
    fn run(&self, db: &dyn Db, mir: &MIR) -> MIR {
        DCECtx::new(db, mir).run()
    }
}

pub struct DCECtx<'a> {
    db: &'a dyn Db,
    mir: &'a MIR,
    block_map: HashMap<MIRBlockID, MIRBlockID>,
    b: MIRBuilder<'a>,
}

impl<'a> DCECtx<'a> {
    pub fn new(db: &'a dyn Db, mir: &'a MIR) -> Self {
        let entry_name = mir.blocks[mir.entry].name.clone();
        let b = MIRBuilder::new(db, entry_name);
        let block_map = HashMap::from([(mir.entry, b.entry)]);
        Self {
            db,
            mir,
            block_map,
            b,
        }
    }

    pub fn run(mut self) -> MIR {
        todo!()
    }
}
