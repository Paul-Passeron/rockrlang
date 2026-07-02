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

use std::collections::HashSet;

use crate::{
    Db,
    common::arena::Arena,
    mir::{
        BasicBlock, BlockID, Local, LocalID, MIR, Terminator,
        basic_block::Stmt, cache::MIRCache,
    },
    thir_to_mir::FuncInst,
};

pub struct MIRBuilder<'a> {
    pub db: &'a dyn Db,
    pub blocks: Arena<BasicBlock>,
    pub locals: Arena<Local>,
    pub entry: BlockID,
    pub parameters: Option<Vec<LocalID>>,

    current_block: BlockID,

    finalized_blocks: HashSet<BlockID>,
}

impl<'a> MIRBuilder<'a> {
    fn current_mut(&mut self) -> &mut BasicBlock {
        self.blocks.get_mut(self.current_block)
    }

    fn current(&self) -> &BasicBlock {
        self.blocks.get(self.current_block)
    }

    pub fn new(db: &'a dyn Db, entry_name: Option<String>) -> Self {
        let entry_block = BasicBlock::empty(entry_name);
        let arena = Arena::new();
        let entry_id = arena.insert(entry_block);
        Self {
            db,
            blocks: arena,
            locals: Arena::new(),
            entry: entry_id,
            parameters: None,
            current_block: entry_id,
            finalized_blocks: HashSet::new(),
        }
    }

    pub fn terminate(&mut self, terminator: Terminator) -> Option<()> {
        if !self.finalized_blocks.insert(self.current_block) {
            return None;
        }
        self.current_mut().terminator = terminator;
        Some(())
    }

    pub fn switch_to_block(&mut self, block: BlockID) -> Option<()> {
        if self.finalized_blocks.contains(&block) {
            return None;
        }
        self.current_block = block;
        Some(())
    }

    pub fn emit(&mut self, stmt: Stmt) {
        // Maybe do these checks only in debug:
        if self.finalized_blocks.contains(&self.current_block) {
            panic!(
                "Writing in a finalized block{}.",
                match &self.current().name {
                    Some(s) => format!(" (current block is `{}`", s),
                    _ => String::new(),
                }
            )
        }
        self.current_mut().stmts.push(stmt);
    }

    pub fn new_block(&self, name: Option<String>) -> BlockID {
        self.blocks.insert(BasicBlock::empty(name))
    }

    pub fn new_local(&self, local: Local) -> LocalID {
        self.locals.insert(local)
    }

    pub fn current_block(&self) -> BlockID {
        self.current_block
    }

    pub fn is_terminated(&self, block: BlockID) -> bool {
        self.finalized_blocks.contains(&block)
    }

    pub fn set_parameters(&mut self, parameters: Vec<LocalID>) -> Option<()> {
        if self.parameters.is_some() {
            return None;
        }
        self.parameters = Some(parameters);
        Some(())
    }

    pub fn finalize(mut self, func: FuncInst) -> Option<MIR> {
        if self.finalized_blocks.len() != self.blocks.len() {
            eprintln!("Not all blocks were finalized: ");
            for (id, bl) in self.blocks.iter() {
                if self.finalized_blocks.contains(&id) {
                    continue;
                }
                eprintln!("{:?}", bl.name);
            }
            return None;
        }

        let parameters = self.parameters.take()?;

        Some(MIR {
            func,
            blocks: self.blocks,
            locals: self.locals,
            entry: self.entry,
            parameters,
            cache: MIRCache::empty(),
        })
    }
}
