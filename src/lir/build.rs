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
    layout::{
        AggregateLayout, Discriminant, IntWidth, LIRTy, LayoutData, LayoutID,
        ScalarKind, VariantsLayout,
    },
    lir::{
        Aggregate, ArithBinop, Body, Branded, Building, CmpBinop, FunctionSig,
        Int, IntValue, LIRDef, LIRFunctionId, Logic, Module, Scalar,
        ScalarMarker, ScalarValue, SigKind, Typed, TypedPtr, Union, ValueClass,
        ValueDef, ValueId, ValueKind, VerifyError,
        branded::{
            BrandedBlockData, BrandedBlockId, BrandedStackSlot, InProgressBody,
        },
        inst::{
            BlockTarget, CastKind, ConstValue, Terminator::Return,
            ValueInstKind, VoidInstKind,
        },
    },
    ril::ptr_of,
};

type Instruction<'ir> = super::inst::Instruction<Branded<'ir>>;
type Terminator<'ir> = super::inst::Terminator<Branded<'ir>>;

pub struct FunctionBuilder<'ir, 'm> {
    db: &'m dyn Db,
    pub id: LIRFunctionId,
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
            body: InProgressBody::new(guard, db, id, &sigs[id.0]),
        };
        f(&mut builder);
        let body = builder.body.finalize()?;
        let slot = match &mut bodies[id.0] {
            Body::Defined(_, Some(_)) => {
                panic!("Cannot write multiple bodies for the same function")
            }
            Body::Defined(_, slot @ None) => slot,
            Body::Import { .. } => {
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
    pub fn build_block<'b, T>(
        &'b mut self,
        id: BrandedBlockId<'ir>,
        f: impl FnOnce(BlockBuilder<'ir, 'b>) -> Terminated<T>,
    ) -> T {
        let Terminated(t) = f(BlockBuilder {
            body: &mut self.body,
            id,
            insts: Vec::new(),
            sigs: self.sigs,
            db: self.db,
        });
        t
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
    fn typed<V: ValueKind>(
        self,
        ty: LIRTy,
        db: &dyn Db,
    ) -> Option<Typed<'ir, V>> {
        if V::matches(ty.class(db)) {
            Some(Typed { id: self, ty, _k: PhantomData })
        } else {
            None
        }
    }

    fn typed_ptr<V: ValueKind>(
        self,
        pointee: LIRTy,
        db: &dyn Db,
    ) -> Option<TypedPtr<'ir, V>> {
        if V::matches(pointee.class(db)) {
            Some(TypedPtr { raw: self, pointee, _k: PhantomData })
        } else {
            None
        }
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

    pub fn store(&mut self, ptr: ValueId<'ir>, value: ValueId<'ir>) {
        debug_assert_eq!(
            self.body.defs[ptr.idx].ty.class(self.db),
            ValueClass::Scalar(ScalarKind::Ptr)
        );
        self.push_void(VoidInstKind::Store { ptr, value })
    }

    pub fn store_typed<V: ValueKind>(
        &mut self,
        ptr: TypedPtr<'ir, V>,
        value: Typed<'ir, V>,
    ) {
        debug_assert_eq!(ptr.pointee.layout, value.ty.layout);
        self.store(ptr.erase(), value.erase())
    }

    pub fn memcpy(&mut self, src: ValueId<'ir>, dst: ValueId<'ir>, ty: LIRTy) {
        debug_assert!(self.body.defs[src.idx].ty.is_ptr(self.db));
        debug_assert!(self.body.defs[dst.idx].ty.is_ptr(self.db));
        self.push_void(VoidInstKind::MemCopy { src, dst, ty });
    }

    pub fn memcpy_typed<K: ValueKind>(
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

    pub fn set_discriminant(&mut self, ptr: ValueId<'ir>, ty: LIRTy, idx: u32) {
        debug_assert!(self.body.defs[ptr.idx].ty.is_ptr(self.db));
        debug_assert!(ty.is_union(self.db));
        debug_assert!(
            ty.union_layout(self.db)
                .map_or(false, |vs| vs.variants.len() > idx as usize)
        );
        self.push_void(VoidInstKind::SetDiscriminant { ptr, ty, idx });
    }

    pub fn set_discriminant_typed(
        &mut self,
        ptr: TypedPtr<'ir, Union>,
        idx: u32,
    ) {
        self.set_discriminant(ptr.erase(), ptr.pointee, idx);
    }

    // Value inst kind

    // constants

    pub fn const_int(&mut self, ty: LIRTy, v: u128) -> IntValue<'ir> {
        debug_assert!(matches!(
            ty.class(self.db),
            ValueClass::Scalar(ScalarKind::Int(_))
        ));
        let val = self.push_value(
            ty,
            ValueInstKind::Const(ConstValue::Int { ty, value: v }),
        );
        IntValue { id: val, ty, _k: PhantomData }
    }

    pub fn const_null_ptr<K: ValueKind>(
        &mut self,
        pointee: LIRTy,
    ) -> TypedPtr<'ir, K> {
        debug_assert!(K::matches(pointee.class(self.db)));
        let ptr_ty = self.ptr_of(pointee);
        let val = self.push_value(
            ptr_ty,
            ValueInstKind::Const(ConstValue::NullPtr { pointee }),
        );
        TypedPtr { raw: val, pointee, _k: PhantomData }
    }

    pub fn zeroed(&mut self, ty: LIRTy) -> ValueId<'ir> {
        debug_assert!(matches!(
            ty.class(self.db),
            ValueClass::Aggregate | ValueClass::None | ValueClass::Union
        ));

        self.push_value(ty, ValueInstKind::Const(ConstValue::Zeroed { ty }))
    }

    pub fn function_ptr(&mut self, id: LIRFunctionId) -> ValueId<'ir> {
        self.push_value(
            LIRTy { layout: LayoutID::ptr(self.db), origin: None },
            ValueInstKind::Const(ConstValue::FunctionAddr(id)),
        )
    }

    pub fn strlit(
        &mut self,
        contents: impl ToString,
        null_terminated: bool,
    ) -> ValueId<'ir> {
        // This is a ptr to the start of the strlit
        self.push_value(
            LIRTy { layout: LayoutID::ptr(self.db), origin: None },
            ValueInstKind::Const(ConstValue::Strlit {
                contents: contents.to_string(),
                null_terminated,
            }),
        )
    }

    // Memory

    pub fn load(&mut self, ptr: ValueId<'ir>, ty: LIRTy) -> ValueId<'ir> {
        self.push_value(ty, ValueInstKind::Load { ptr, ty })
    }

    pub fn load_typed<K: ValueKind>(
        &mut self,
        ptr: TypedPtr<'ir, K>,
    ) -> Typed<'ir, K> {
        self.load(ptr.raw, ptr.pointee).typed(ptr.pointee, self.db).unwrap()
    }

    // Addressing

    pub fn field_ptr(
        &mut self,
        ptr: ValueId<'ir>,
        ty: LIRTy,
        src_idx: u32,
    ) -> ValueId<'ir> {
        debug_assert!(self.body.defs[ptr.idx].ty.is_ptr(self.db));
        debug_assert!(ty.is_aggregate(self.db));
        debug_assert!(
            ty.aggregate_layout(self.db)
                .and_then(|l| l.source_field(src_idx))
                .is_some()
        );
        let ptr_ty = self.ptr_of(ty);
        self.push_value(ptr_ty, ValueInstKind::FieldPtr { ptr, ty, src_idx })
    }

    pub fn field_ptr_typed<K: ValueKind>(
        &mut self,
        ptr: TypedPtr<'ir, Aggregate>,
        src_idx: u32,
    ) -> TypedPtr<'ir, K> {
        let val = self.field_ptr(ptr.raw, ptr.pointee, src_idx);
        let agg_layout = ptr.layout(self.db);

        let field_layout = agg_layout.source_field(src_idx).unwrap().1;
        let field_ty = LIRTy { layout: field_layout, origin: None };
        val.typed_ptr(field_ty, self.db).unwrap()
    }

    pub fn union_payload_ptr(
        &mut self,
        ptr: ValueId<'ir>,
        ty: LIRTy,
        variant: u32,
    ) -> ValueId<'ir> {
        self.push_value(
            self.opaque_ptr_ty(),
            ValueInstKind::UnionPayloadPtr { ptr, ty, variant },
        )
    }

    pub fn union_payload_ptr_typed<K: ValueKind>(
        &mut self,
        ptr: TypedPtr<'ir, Union>,
        variant: u32,
    ) -> TypedPtr<'ir, K> {
        let val = self.union_payload_ptr(ptr.raw, ptr.pointee, variant);
        let payload_layout = ptr.layout(self.db).variants[variant as usize];
        let payload_ty = LIRTy { layout: payload_layout, origin: None };
        val.typed_ptr(payload_ty, self.db).unwrap()
    }

    pub fn get_discriminant(
        &mut self,
        ptr: ValueId<'ir>,
        ty: LIRTy,
    ) -> Option<Typed<'ir, Scalar<Int>>> {
        let discr = &ty.union_layout(self.db).unwrap().discriminant;
        let discr_layout = match discr {
            Discriminant::None => LayoutID::zst(self.db),
            Discriminant::Tagged { kind, .. } => LayoutID::int(self.db, *kind),
            Discriminant::Niche { .. } => todo!(),
        };
        let discr_ty = LIRTy { layout: discr_layout, origin: None };

        let val = self
            .push_value(discr_ty, ValueInstKind::GetDiscriminant { ptr, ty });
        val.typed(discr_ty, self.db)
    }

    pub fn get_discriminant_typed(
        &mut self,
        ptr: TypedPtr<'ir, Union>,
    ) -> Option<Typed<'ir, Scalar<Int>>> {
        self.get_discriminant(ptr.erase(), ptr.pointee)
    }

    // Aggregates

    pub fn make_aggregate(
        &mut self,
        ty: LIRTy,
        // fields in source order
        fields_in_src_order: Vec<ValueId<'ir>>,
    ) -> Typed<'ir, Aggregate> {
        debug_assert!(ty.is_aggregate(self.db));
        // TODO: check fields
        self.push_value(
            ty,
            ValueInstKind::MakeAggregate { ty, fields_in_src_order },
        )
        .typed(ty, self.db)
        .unwrap()
    }

    pub fn extract_field(
        &mut self,
        value: ValueId<'ir>,
        ty: LIRTy,
        src_idx: u32,
    ) -> ValueId<'ir> {
        debug_assert!(ty.is_aggregate(self.db));
        debug_assert_eq!(self.body.defs[value.idx].ty.layout, ty.layout);
        debug_assert!(
            ty.aggregate_layout(self.db)
                .and_then(|l| l.source_field(src_idx))
                .is_some()
        );
        let field_layout = ty
            .aggregate_layout(self.db)
            .unwrap()
            .source_field(src_idx)
            .unwrap()
            .1;
        let field_ty = LIRTy { layout: field_layout, origin: None };
        self.push_value(
            field_ty,
            ValueInstKind::ExtractField { value, ty, src_idx },
        )
    }

    pub fn extract_field_typed(
        &mut self,
        value: Typed<'ir, Aggregate>,
        src_idx: u32,
    ) -> ValueId<'ir> {
        self.extract_field(value.erase(), value.ty, src_idx)
    }

    pub fn insert_field(
        &mut self,
        value: ValueId<'ir>,
        ty: LIRTy,
        src_idx: u32,
        field: ValueId<'ir>,
    ) -> ValueId<'ir> {
        debug_assert!(ty.is_aggregate(self.db));
        debug_assert_eq!(self.body.defs[value.idx].ty.layout, ty.layout);
        debug_assert!(
            ty.aggregate_layout(self.db)
                .and_then(|l| l.source_field(src_idx))
                .is_some()
        );
        debug_assert_eq!(
            ty.aggregate_layout(self.db)
                .unwrap()
                .source_field(src_idx)
                .unwrap()
                .1,
            self.body.defs[field.idx].ty.layout
        );
        self.push_value(
            ty,
            ValueInstKind::InsertField { value, ty, src_idx, field },
        )
    }

    pub fn insert_field_typed(
        &mut self,
        value: Typed<'ir, Aggregate>,
        src_idx: u32,
        field: ValueId<'ir>,
    ) -> ValueId<'ir> {
        self.insert_field(value.erase(), value.ty, src_idx, field)
    }

    // Computation

    // TODO: typed and untyped helpers for each operation

    pub fn arith(
        &mut self,
        op: ArithBinop,
        lhs: ValueId<'ir>,
        rhs: ValueId<'ir>,
    ) -> ValueId<'ir> {
        let lhs_ty = self.body.defs[lhs.idx].ty;
        let rhs_ty = self.body.defs[rhs.idx].ty;
        debug_assert_eq!(lhs_ty.layout, rhs_ty.layout);
        debug_assert!(lhs_ty.is_int(self.db));
        let ty = self.combine(lhs_ty, rhs_ty);
        self.push_value(ty, ValueInstKind::Arith { op, lhs, rhs })
    }

    pub fn arith_typed(
        &mut self,
        op: ArithBinop,
        lhs: Typed<'ir, Scalar<Int>>,
        rhs: Typed<'ir, Scalar<Int>>,
    ) -> Typed<'ir, Scalar<Int>> {
        self.arith(op, lhs.erase(), rhs.erase()).typed(lhs.ty, self.db).unwrap()
    }

    pub fn cmp(
        &mut self,
        op: CmpBinop,
        lhs: ValueId<'ir>,
        rhs: ValueId<'ir>,
    ) -> ValueId<'ir> {
        let lhs_ty = self.body.defs[lhs.idx].ty;
        let rhs_ty = self.body.defs[rhs.idx].ty;
        debug_assert_eq!(lhs_ty.layout, rhs_ty.layout);
        debug_assert!(lhs_ty.is_scalar(self.db));
        self.push_value(self.bool_ty(), ValueInstKind::Cmp { op, lhs, rhs })
    }

    pub fn cmp_typed(
        &mut self,
        op: CmpBinop,
        lhs: Typed<'ir, Scalar<Int>>,
        rhs: Typed<'ir, Scalar<Int>>,
    ) -> Typed<'ir, Scalar<Int>> {
        self.cmp(op, lhs.erase(), rhs.erase())
            .typed(self.bool_ty(), self.db)
            .unwrap()
    }

    pub fn logic(
        &mut self,
        op: Logic,
        lhs: ValueId<'ir>,
        rhs: ValueId<'ir>,
    ) -> ValueId<'ir> {
        let bool_ty = self.bool_ty();
        debug_assert_eq!(self.body.defs[lhs.idx].ty.layout, bool_ty.layout);
        debug_assert_eq!(self.body.defs[rhs.idx].ty.layout, bool_ty.layout);
        self.push_value(bool_ty, ValueInstKind::Logic { op, lhs, rhs })
    }

    pub fn logic_typed(
        &mut self,
        op: Logic,
        lhs: IntValue<'ir>,
        rhs: IntValue<'ir>,
    ) -> IntValue<'ir> {
        debug_assert_eq!(lhs.width(self.db), IntWidth::I8);
        debug_assert_eq!(rhs.width(self.db), IntWidth::I8);
        self.logic(op, lhs.erase(), rhs.erase())
            .typed(self.bool_ty(), self.db)
            .unwrap()
    }

    pub fn not(&mut self, value: ValueId<'ir>) -> ValueId<'ir> {
        let bool_ty = self.bool_ty();
        debug_assert_eq!(self.body.defs[value.idx].ty.layout, bool_ty.layout);
        self.push_value(bool_ty, ValueInstKind::Not { value })
    }

    pub fn not_typed(&mut self, value: IntValue<'ir>) -> IntValue<'ir> {
        let bool_ty = self.bool_ty();
        self.not(value.erase()).typed(bool_ty, self.db).unwrap()
    }

    pub fn cast(
        &mut self,
        kind: CastKind,
        value: ValueId<'ir>,
        to: ScalarKind,
    ) -> ValueId<'ir> {
        debug_assert!(self.body.defs[value.idx].ty.is_scalar(self.db));
        let ty = LIRTy { layout: LayoutID::scalar(self.db, to), origin: None };
        self.push_value(ty, ValueInstKind::Cast { kind, value, to })
    }

    pub fn bitcast(
        &mut self,
        value: ValueId<'ir>,
        to: ScalarKind,
    ) -> ValueId<'ir> {
        self.cast(CastKind::Bitcast, value, to)
    }

    pub fn cast_ptr<S: ScalarMarker, K: ValueKind>(
        &mut self,
        value: ScalarValue<'ir, S>,
        pointee: LIRTy,
    ) -> TypedPtr<'ir, K>
    where
        Scalar<S>: ValueKind,
    {
        let kind = match value.kind(self.db) {
            ScalarKind::Int(_) => CastKind::IntToPtr,
            ScalarKind::Ptr => CastKind::Bitcast,
            ScalarKind::Float(_) => CastKind::Bitcast,
        };
        self.cast(kind, value.erase(), ScalarKind::Ptr)
            .typed_ptr(pointee, self.db)
            .unwrap()
    }

    // Call

    pub fn call(
        &mut self,
        f: LIRFunctionId,
        args: Vec<ValueId<'ir>>,
    ) -> Option<ValueId<'ir>> {
        let ret_ty = self.get_ret_ty(f);
        let dest = if ret_ty.is_zst(self.db) {
            None
        } else {
            let idx = self.body.defs.insert(LIRDef { ty: ret_ty });
            let def = ValueDef { idx, _brand: std::marker::PhantomData };
            Some(def)
        };

        let res = dest.as_ref().map(|d| d.id());

        let params = self.get_params(f);

        // assert_eq!(params.len(), args.len());

        for (param, arg) in params.iter().zip(args.iter()) {
            assert_eq!(param.layout, self.body.defs[arg.idx].ty.layout);
        }

        self.insts.push(Instruction::Call { dest, id: f, args });

        res
    }

    // Block utils

    pub fn target(
        &self,
        block: BrandedBlockId<'ir>,
        params: Vec<ValueId<'ir>>,
    ) -> BlockTarget<Branded<'ir>> {
        BlockTarget { params, block }
    }

    // Terminators

    pub fn goto(self, target: BlockTarget<Branded<'ir>>) -> Terminated<()> {
        self.terminate(Terminator::Goto(target))
    }

    pub fn br(
        self,
        cond: ValueId<'ir>,
        if_true: BlockTarget<Branded<'ir>>,
        if_false: BlockTarget<Branded<'ir>>,
    ) -> Terminated<()> {
        self.terminate(Terminator::Br { cond, if_true, if_false })
    }

    pub fn switch(
        self,
        on: ValueId<'ir>,
        branches: Vec<(u128, BlockTarget<Branded<'ir>>)>,
        default: BlockTarget<Branded<'ir>>,
    ) -> Terminated<()> {
        self.terminate(Terminator::Switch { on, branches, default })
    }

    fn get_sig(&self, id: LIRFunctionId) -> &'b FunctionSig {
        &self.sigs[id.0]
    }

    fn get_ret_ty(&self, id: LIRFunctionId) -> LIRTy {
        self.get_sig(id).signature.ret
    }

    fn get_params(&self, id: LIRFunctionId) -> &'b [LIRTy] {
        &self.get_sig(id).signature.params
    }

    pub fn ret(self, value: Option<ValueId<'ir>>) -> Terminated<()> {
        let ret_ty = self.get_ret_ty(self.body.id);
        match value {
            Some(v) => {
                assert_eq!(self.body.defs[v.idx].ty.layout, ret_ty.layout)
            }
            None => assert!(ret_ty.is_zst(self.db)),
        }
        self.terminate(Return(value))
    }

    pub fn diverge(self) -> Terminated<()> {
        self.terminate(Terminator::Diverge)
    }

    // Utils:
    fn bool_ty(&self) -> LIRTy {
        LIRTy { layout: LayoutID::int(self.db, IntWidth::I8), origin: None }
    }

    fn ptr_of(&self, ty: LIRTy) -> LIRTy {
        LIRTy {
            layout: LayoutID::ptr(self.db),
            origin: ty.origin.map(|ty| ptr_of(self.db, ty, true).into()),
        }
    }

    fn combine(&self, a: LIRTy, b: LIRTy) -> LIRTy {
        debug_assert_eq!(a.layout, b.layout);
        let origin = a.origin.or(b.origin);
        LIRTy { layout: a.layout, origin }
    }

    fn opaque_ptr_ty(&self) -> LIRTy {
        LIRTy { layout: LayoutID::ptr(self.db), origin: None }
    }

    pub fn type_of(&self, id: ValueId<'ir>) -> LIRTy {
        self.body.defs[id.idx].ty
    }
}

impl<T> Terminated<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Terminated<U> {
        let Terminated(value) = self;
        Terminated(f(value))
    }

    pub fn take(self) -> (T, Terminated<()>) {
        let Terminated(t) = self;
        (t, Terminated(()))
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

    pub fn is_ptr(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Scalar(ScalarKind::Ptr))
    }

    pub fn is_union(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Union(_))
    }

    pub fn union_layout(self, db: &dyn Db) -> Option<&VariantsLayout> {
        match self.layout.data(db) {
            LayoutData::Union(variants_layout) => Some(variants_layout),
            _ => None,
        }
    }

    pub fn is_aggregate(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Aggregate(_))
    }

    pub fn aggregate_layout(self, db: &dyn Db) -> Option<&AggregateLayout> {
        match self.layout.data(db) {
            LayoutData::Aggregate(aggregate_layout) => Some(aggregate_layout),
            _ => None,
        }
    }

    pub fn is_int(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Scalar(ScalarKind::Int(_)))
    }

    pub fn is_scalar(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Scalar(_))
    }

    pub fn scalar(self, db: &dyn Db) -> Option<ScalarKind> {
        match self.layout.data(db) {
            LayoutData::Scalar(kind) => Some(*kind),
            _ => None,
        }
    }
}

impl<'ir> TypedPtr<'ir, Aggregate> {
    pub fn layout(self, db: &dyn Db) -> &AggregateLayout {
        self.pointee.aggregate_layout(db).unwrap()
    }
}

impl<'ir> TypedPtr<'ir, Union> {
    pub fn layout(self, db: &dyn Db) -> &VariantsLayout {
        self.pointee.union_layout(db).unwrap()
    }
}

impl<'ir> TypedPtr<'ir, Scalar<Int>> {
    pub fn layout(self, db: &dyn Db) -> IntWidth {
        match self.pointee.layout.data(db) {
            LayoutData::Scalar(ScalarKind::Int(width)) => *width,
            _ => unreachable!(),
        }
    }
}

impl<'ir> IntValue<'ir> {
    pub fn width(&self, db: &dyn Db) -> IntWidth {
        match self.ty.layout.data(db) {
            LayoutData::Scalar(ScalarKind::Int(width)) => *width,
            _ => unreachable!(),
        }
    }
}

impl<'ir> InProgressBody<'ir> {
    pub fn new_block_with_params(
        &self,
        name: Option<Symbol>,
        params: &[LIRTy],
    ) -> (BrandedBlockId<'ir>, Vec<ValueId<'ir>>) {
        let defs: Vec<ValueDef<'ir>> = params
            .iter()
            .map(|ty| ValueDef {
                idx: self.defs.insert(LIRDef { ty: *ty }),
                _brand: PhantomData,
            })
            .collect();
        let ids = defs.iter().map(|d| d.id()).collect();
        let idx = self.blocks.insert(BrandedBlockData {
            name,
            params: defs,
            insts: Vec::new(),
            terminator: None,
        });
        let id = BrandedBlockId { idx, _brand: PhantomData };
        (id, ids)
    }
}

impl<'ir, 'm> FunctionBuilder<'ir, 'm> {
    pub fn param(&self, i: usize) -> (LIRTy, ValueId<'ir>) {
        let def = &self.body.blocks[self.body.entry.idx].params[i];
        (self.body.defs[def.idx].ty, def.id())
    }

    pub fn stack_slot(&mut self, ty: LIRTy) -> ValueId<'ir> {
        assert!(!ty.is_zst(self.db));
        let idx = self.body.defs.insert(LIRDef {
            ty: LIRTy { layout: LayoutID::ptr(self.db), origin: None },
        });
        let value = ValueId { idx, _brand: PhantomData };
        let slot = BrandedStackSlot { value, ty };
        self.body.slots.push(slot);
        value
    }

    pub fn stack_slot_typed<K: ValueKind>(
        &mut self,
        ty: LIRTy,
    ) -> TypedPtr<'ir, K> {
        assert!(K::matches(ty.class(self.db)));
        self.stack_slot(ty).typed_ptr(ty, self.db).unwrap()
    }
}

impl<'ir, S: ScalarMarker> Typed<'ir, Scalar<S>>
where
    Scalar<S>: ValueKind,
{
    pub fn kind(&self, db: &dyn Db) -> ScalarKind {
        self.ty.scalar(db).unwrap()
    }
}
