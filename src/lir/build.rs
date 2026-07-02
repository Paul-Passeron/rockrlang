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

use std::marker::PhantomData;

use crate::{
    Db,
    common::symbols::Symbol,
    layout::{LIRTy, LayoutData, LayoutID, ScalarKind},
    lir::{
        Body, Branded, Building, FunctionSig, Int, IntValue, LIRDef,
        LIRFunctionId, Module, SigKind, Signature, Typed, TypedPtr, ValueClass,
        ValueDef, ValueId, ValueKind, VerifyError,
        branded::{BrandedBlockId, InProgressBody},
        inst::{BlockTarget, ConstValue, ValueInstKind, VoidInstKind},
    },
    ril::ptr_of,
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

pub struct Terminated<T>(T);

pub struct BlockBuilder<'ir, 'b> {
    body: &'b mut InProgressBody<'ir>,
    id: BrandedBlockId<'ir>,
    insts: Vec<Instruction<'ir>>,

    sigs: &'b [FunctionSig],
    db: &'b dyn Db,
}

impl<'ir, 'b> BlockBuilder<'ir, 'b> {
    pub fn terminate(self, term: Terminator<'ir>) -> Terminated<()> {
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

impl<'ir, 'm> FunctionBuilder<'ir, 'm> {
    pub fn build_block<'b>(
        &'b mut self,
        id: BrandedBlockId<'ir>,
        f: impl FnOnce(BlockBuilder<'ir, 'b>) -> Terminated<()>,
    ) {
        let Terminated(()) = f(BlockBuilder {
            body: &mut self.body,
            id,
            insts: Vec::new(),
            sigs: self.sigs,
            db: self.db,
        });
    }
}

impl<'ir, 'm> FunctionBuilder<'ir, 'm> {
    pub fn new_block(&self, name: Option<Symbol>) -> BrandedBlockId<'ir> {
        self.body.new_block(name)
    }
}

impl<'ir, K: ValueKind> TypedPtr<'ir, K> {
    pub fn erase(self) -> ValueId<'ir> {
        self.raw
    }
}

impl<'ir> ValueId<'ir> {
    pub fn typed<V: ValueKind>(self, ty: LIRTy) -> Option<Typed<'ir, V>> {
        todo!()
    }

    pub fn typed_ptr<V: ValueKind>(
        self,
        ty: LIRTy,
    ) -> Option<TypedPtr<'ir, V>> {
        todo!()
    }
}

impl<'ir, 'b> BlockBuilder<'ir, 'b> {
    fn push_value(
        &mut self,
        ty: LIRTy,
        kind: ValueInstKind<Branded<'ir>>,
    ) -> ValueId<'ir> {
        let idx = self.body.defs.insert(LIRDef { ty });
        let dest = ValueDef { idx, _brand: std::marker::PhantomData };
        let id = dest.id();
        self.insts.push(Instruction::Value { def: dest, kind });
        id
    }

    fn push_void(&mut self, kind: VoidInstKind<Branded<'ir>>) {
        self.insts.push(Instruction::Void(kind));
    }

    // Void inst kind

    fn store(&mut self, ptr: ValueId<'ir>, value: ValueId<'ir>) {
        debug_assert_eq!(
            self.body.defs[ptr.idx].ty.class(self.db),
            ValueClass::Scalar(ScalarKind::Ptr)
        );
        self.push_void(VoidInstKind::Store { ptr, value })
    }

    fn store_typed<V: ValueKind>(
        &mut self,
        ptr: TypedPtr<'ir, V>,
        value: Typed<'ir, V>,
    ) {
        self.store(ptr.erase(), value.erase())
    }

    fn memcpy(&mut self, src: ValueId<'ir>, dst: ValueId<'ir>, ty: LIRTy) {
        debug_assert!(self.body.defs[src.idx].ty.is_ptr(self.db));
        debug_assert!(self.body.defs[dst.idx].ty.is_ptr(self.db));
        self.push_void(VoidInstKind::MemCopy { src, dst, ty });
    }

    fn memcpy_typed<K: ValueKind>(
        &mut self,
        src: TypedPtr<'ir, K>,
        dst: TypedPtr<'ir, K>,
    ) {
        debug_assert_eq!(src.pointee.layout, dst.pointee.layout);
        let LIRTy { origin: src_orig, .. } = src.pointee;
        let LIRTy { origin: dst_orig, .. } = dst.pointee;
        let origin = src_orig.or(dst_orig);
        self.memcpy(
            src.erase(),
            dst.erase(),
            LIRTy { layout: src.pointee.layout, origin },
        );
    }

    // Value inst kind

    // constants

    fn const_int(&mut self, ty: LIRTy, v: u128) -> IntValue<'ir> {
        debug_assert!(matches!(
            ty.class(self.db),
            ValueClass::Scalar(ScalarKind::Int(_))
        ));
        let val = self.push_value(
            ty,
            ValueInstKind::Const(ConstValue::Int { ty, value: v }),
        );
        IntValue { id: val, _k: PhantomData }
    }

    fn const_null_ptr<K: ValueKind>(
        &mut self,
        pointee: LIRTy,
    ) -> TypedPtr<'ir, K> {
        debug_assert!(K::matches(pointee.class(self.db)));
        let ptr_ty = LIRTy {
            layout: LayoutID::ptr(self.db),
            origin: pointee.origin.map(|ty| ptr_of(self.db, ty, false).into()),
        };

        let val = self.push_value(
            ptr_ty,
            ValueInstKind::Const(ConstValue::NullPtr { pointee }),
        );
        TypedPtr { raw: val, pointee, _k: PhantomData }
    }

    fn zeroed(&mut self, ty: LIRTy) -> ValueId<'ir> {
        debug_assert!(matches!(
            ty.class(self.db),
            ValueClass::Aggregate | ValueClass::None | ValueClass::Union
        ));

        self.push_value(ty, ValueInstKind::Const(ConstValue::Zeroed { ty }))
    }

    fn function_ptr(&mut self, id: LIRFunctionId) -> ValueId<'ir> {
        self.push_value(
            LIRTy { layout: LayoutID::ptr(self.db), origin: None },
            ValueInstKind::Const(ConstValue::FunctionAddr(id)),
        )
    }

    // Memory

    fn alloca(&mut self, ty: LIRTy) -> ValueId<'ir> {
        self.push_value(ty, ValueInstKind::Alloca { ty })
    }

    fn alloca_typed<K: ValueKind>(&mut self, ty: LIRTy) -> TypedPtr<'ir, K> {
        debug_assert!(K::matches(ty.class(self.db)));
        let val = self.alloca(ty);
        TypedPtr { raw: val, pointee: ty, _k: PhantomData }
    }

    fn load(&mut self, ptr: ValueId<'ir>, ty: LIRTy) -> ValueId<'ir> {
        self.push_value(ty, ValueInstKind::Load { ptr, ty })
    }

    fn load_typed<K: ValueKind>(
        &mut self,
        ptr: TypedPtr<'ir, K>,
    ) -> Typed<'ir, K> {
        debug_assert!(K::matches(ptr.pointee.class(self.db)));
        Typed { id: self.load(ptr.raw, ptr.pointee), _k: PhantomData }
    }

    // Terminators

    fn call(
        self,
        f: LIRFunctionId,
        args: Vec<ValueId<'ir>>,
        next_block: BlockTarget<Branded<'ir>>,
    ) -> Terminated<Option<ValueId<'ir>>> {
        let sig = &self.sigs[f.0];
        let ret_ty = sig.signature.ret;
        let dest = if ret_ty.is_zst(self.db) {
            None
        } else {
            let idx = self.body.defs.insert(LIRDef { ty: ret_ty });
            let def = ValueDef { idx, _brand: std::marker::PhantomData };
            Some(def)
        };

        let value = dest.as_ref().map(|x| x.id());

        let t = Terminator::Call { id: f, args, dest, next: next_block };

        self.terminate(t).map(|_| value)
    }
}

impl<T> Terminated<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Terminated<U> {
        let Terminated(value) = self;
        Terminated(f(value))
    }
}

impl LIRTy {
    pub fn class(&self, db: &dyn Db) -> ValueClass {
        match self.layout.data(db) {
            LayoutData::Scalar(kind) => ValueClass::Scalar(*kind),
            LayoutData::Aggregate(_) => ValueClass::Aggregate,
            LayoutData::ZeroSized => ValueClass::None,
            LayoutData::Union(_) => ValueClass::Union,
        }
    }

    pub fn is_ptr(&self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Scalar(ScalarKind::Ptr))
    }
}
