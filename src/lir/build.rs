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

use crate::{
    Db,
    common::symbols::Symbol,
    lir::{
        Body, Branded, Building, FunctionSig, LIRFunctionId, Module, PtrValue,
        SigKind, Typed, ValueId, ValueKind, VerifyError,
        branded::{BrandedBlockId, InProgressBody},
        inst::VoidInstKind,
    },
};

type Instruction<'ir> = super::inst::Instruction<Branded<'ir>>;
type Terminator<'ir> = super::inst::Terminator<Branded<'ir>>;

pub struct FunctionBuilder<'ir, 'm> {
    db: &'m dyn Db,
    id: LIRFunctionId,
    sigs: &'m [FunctionSig],
    pub body: InProgressBody<'ir>,
}

impl Module<Building> {
    pub fn build_function(
        &mut self,
        db: &dyn Db,
        id: LIRFunctionId,
        f: impl FnOnce(&mut FunctionBuilder<'_, '_>),
    ) -> Result<(), VerifyError> {
        let Module { sigs, bodies } = self;
        assert!(matches!(sigs[id.0].kind, SigKind::Defined(_)));
        generativity::make_guard!(guard);
        let mut builder = FunctionBuilder {
            db,
            id,
            sigs,
            body: InProgressBody::new(guard, db, &sigs[id.0]),
        };
        f(&mut builder);
        let body = builder.body.finalize()?;
        let slot = match &mut bodies[id.0] {
            Body::Defined(_, Some(_)) => {
                panic!("Cannot write multiple bodies for the same function")
            }
            Body::Defined(_, slot @ None) => slot,
            Body::Import => {
                panic!("Cannot set the body of an imported function")
            }
        };
        *slot = Some(body);
        Ok(())
    }
}

pub struct Terminated(());

pub struct BlockBuilder<'ir, 'b> {
    body: &'b mut InProgressBody<'ir>,
    id: BrandedBlockId<'ir>,
    insts: Vec<Instruction<'ir>>,
}

impl<'ir, 'b> BlockBuilder<'ir, 'b> {
    pub fn terminate(self, term: Terminator<'ir>) -> Terminated {
        let block = &mut self.body.blocks[self.id.idx];
        block.insts = self.insts;
        let slot = match &mut block.terminator {
            Some(_) => panic!("Cannot finalize a block twice !"),
            slot @ None => slot,
        };
        *slot = Some(term);
        Terminated(())
    }
}

impl<'ir> InProgressBody<'ir> {
    pub fn build_block<'b>(
        &'b mut self,
        id: BrandedBlockId<'ir>,
        f: impl FnOnce(BlockBuilder<'ir, 'b>) -> Terminated,
    ) {
        let Terminated(()) =
            f(BlockBuilder { body: self, id, insts: Vec::new() });
    }
}

impl<'ir, 'm> FunctionBuilder<'ir, 'm> {
    pub fn new_block(&self, name: Option<Symbol>) -> BrandedBlockId<'ir> {
        self.body.new_block(name)
    }
}

impl<'ir, 'b> BlockBuilder<'ir, 'b> {
    // Used for instructions where you don't have a guarantee on the instruction
    // result
    fn inst(&mut self, inst: Instruction<'ir>) -> Option<ValueId<'ir>> {
        let res = match &inst {
            Instruction::Void(_) => None,
            Instruction::Value { def, .. } => Some(def.id()),
        };
        self.insts.push(inst);
        res
    }

    fn store<V: ValueKind>(
        &mut self,
        ptr: PtrValue<'ir, V>,
        value: Typed<'ir, V>,
    ) {
        let inst = Instruction::Void(VoidInstKind::Store {
            ptr: ptr.erase(),
            value: value.erase(),
        });
        self.insts.push(inst);
    }
}
