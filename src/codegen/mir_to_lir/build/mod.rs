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

use std::{collections::HashMap, ops::Index};

use itertools::Itertools;

use crate::{
    Db,
    codegen::{Codegen, LIRToLLVM, MIRToLIRBuild},
    common::symbols::Symbol,
    layout::{IntWidth, LIRTy, LayoutData, LayoutID, layout_of},
    lir::{
        ArithBinop, Body, CmpBinop, LIRFunctionId, ValueId,
        branded::BrandedBlockId,
        build::{BlockBuilder, FunctionBuilder, Terminated},
        inst::Terminator,
    },
    mir::{
        MIR, MIRBlockID, MIRLocalID,
        basic_block::{MIRBasicBlock, MIRTerminator, Stmt},
        operand::{
            MIRCallee, MIRConstant, MIRConstructorArgs, MIROperand, MIRPlace,
            MIRProjection, MIRRValue, MIRRValueKind, UnaryOperator,
        },
    },
    name_resolve::type_expr::struct_item,
    parse_tree::expr::BinaryOperator,
    ril::{TypeRef, bool_id},
    thir_to_mir::FuncInst,
};

pub struct MIRMap<'a> {
    mir_to_lir: HashMap<FuncInst, LIRFunctionId>,
    mirs: HashMap<FuncInst, &'a MIR>,
    lir_to_mir: HashMap<LIRFunctionId, FuncInst>,
}

pub struct MTLBCtx<'a> {
    pub db: &'a dyn Db,
    pub mir_map: MIRMap<'a>,
}

#[allow(unused)]
pub enum LocalSlot<'ir> {
    Zst,
    Ptr { ptr: ValueId<'ir>, ty: LIRTy },
    Value(ValueId<'ir>), /* TODO: analysis that figures out which locals we
                          * can use here */
}

struct LIRLower<'ir, 'db> {
    mir: &'db MIR,
    block_map: HashMap<MIRBlockID, BrandedBlockId<'ir>>,
    value_map: HashMap<MIRLocalID, LocalSlot<'ir>>,
}

#[allow(unused)]
enum ProjKind<'a> {
    Regular(&'a MIRProjection),
    DowncastThen { variant: u32, next: &'a MIRProjection },
}

impl<'ir> LocalSlot<'ir> {
    fn ptr(&self) -> ValueId<'ir> {
        match self {
            LocalSlot::Ptr { ptr, .. } => *ptr,
            LocalSlot::Zst => panic!("ZST slots have no pointer"),
            _ => unreachable!(),
        }
    }

    fn is_zst(&self) -> bool {
        matches!(self, Self::Zst)
    }
}

impl<'a> MIRMap<'a> {
    pub fn new() -> Self {
        Self {
            mir_to_lir: HashMap::new(),
            mirs: HashMap::new(),
            lir_to_mir: HashMap::new(),
        }
    }

    pub fn add(&mut self, mir: &'a MIR, id: LIRFunctionId) {
        let inst = mir.func;
        self.mir_to_lir.insert(inst, id);
        self.lir_to_mir.insert(id, inst);
        self.mirs.insert(inst, mir);
    }

    pub fn lirs(&self) -> Vec<LIRFunctionId> {
        self.lir_to_mir.keys().copied().collect()
    }

    pub fn add_import(&mut self, inst: FuncInst, id: LIRFunctionId) {
        self.mir_to_lir.insert(inst, id);
        self.lir_to_mir.insert(id, inst);
    }
}

impl<'a> Index<LIRFunctionId> for MIRMap<'a> {
    type Output = &'a MIR;

    fn index(&self, index: LIRFunctionId) -> &Self::Output {
        &self[self.lir_to_mir[&index]]
    }
}

impl<'a> Index<FuncInst> for MIRMap<'a> {
    type Output = &'a MIR;

    fn index(&self, index: FuncInst) -> &Self::Output {
        &self.mirs[&index]
    }
}

impl<'db, 'ctx> Codegen<'db, MIRToLIRBuild<'db, 'ctx>> {
    pub fn finalize(self) -> Codegen<'db, LIRToLLVM<'db, 'ctx>> {
        Codegen { db: self.db, lir: self.lir.finalize(), ctx: () }
    }

    pub fn run(mut self) -> Codegen<'db, LIRToLLVM<'db, 'ctx>> {
        self.ctx.mir_map.lirs().into_iter().for_each(|lir_id| {
            if let Body::Defined(_, _) = self.lir.get_fn(lir_id).1 {
                self.lower(lir_id)
            }
        });
        self.finalize()
    }

    fn lower(&mut self, lir_id: LIRFunctionId) {
        let db = self.db;
        let Self { lir, ctx, .. } = self;
        lir.build_function(db, lir_id, |b| Self::_lower(b, ctx)).unwrap()
    }

    fn _lower<'ir>(builder: &mut FunctionBuilder<'ir, '_>, ctx: &mut MTLBCtx) {
        ctx.lower(builder);
    }
}

impl<'a> MTLBCtx<'a> {
    fn add_block<'ir>(
        &mut self,
        b: &mut FunctionBuilder<'ir, '_>,
        id: MIRBlockID,
        data: &MIRBasicBlock,
        lower: &mut LIRLower<'ir, '_>,
    ) {
        let b = b.new_block(
            data.name.as_ref().map(|name| Symbol::new(self.db, name)),
        );
        lower.block_map.insert(id, b);
    }

    pub fn lower<'ir>(&mut self, b: &mut FunctionBuilder<'ir, '_>) {
        let mir = self.mir_map[b.id];
        let mut lower = LIRLower {
            mir,
            block_map: HashMap::new(),
            value_map: HashMap::new(),
        };
        mir.blocks
            .iter()
            .for_each(|(blk, data)| self.add_block(b, blk, data, &mut lower));
        let params = b.body.blocks[b.body.entry.idx]
            .params
            .iter()
            .map(|p| p.id())
            .collect_vec();
        for (local, decl) in mir.locals.iter() {
            let layout = layout_of(self.db, decl.ty);
            if layout.is_zst(self.db) {
                lower.value_map.insert(local, LocalSlot::Zst);
            } else {
                let ty = LIRTy { layout, origin: Some(decl.ty) };
                let ptr = b.stack_slot(ty);
                lower.value_map.insert(local, LocalSlot::Ptr { ptr, ty });
            }
        }
        b.build_block(b.body.entry, |mut bb| {
            for (i, param_local) in mir
                .parameters
                .iter()
                .filter(|param| {
                    let tref = lower.mir.locals[**param].ty;
                    let layout = layout_of(self.db, tref);
                    !layout.is_zst(self.db)
                })
                .enumerate()
            {
                let slot = &lower.value_map[param_local];
                if !slot.is_zst() {
                    bb.store(slot.ptr(), params[i]);
                }
            }

            let mir_entry_target =
                bb.target(lower.block_map[&mir.entry], vec![]);
            bb.goto(mir_entry_target)
        });
        mir.blocks.keys().for_each(|blk| {
            b.build_block(lower.block_map[&blk], |bb| {
                self.lower_block(bb, blk, &mut lower)
            })
        });
    }

    fn lower_block<'ir>(
        &mut self,
        mut b: BlockBuilder<'ir, '_>,
        blk: MIRBlockID,
        lower: &mut LIRLower<'ir, '_>,
    ) -> Terminated<()> {
        let mir = lower.mir;
        let data = &mir.blocks[blk];
        data.stmts.iter().for_each(|stmt| self.lower_stmt(&mut b, stmt, lower));
        self.lower_terminator(b, &data.terminator, lower)
    }

    fn lower_stmt<'ir>(
        &mut self,
        b: &mut BlockBuilder<'ir, '_>,
        stmt: &Stmt,
        lower: &LIRLower<'ir, '_>,
    ) {
        match stmt {
            Stmt::Assign { dest, rvalue } => {
                if !lower.value_map[&dest.local].is_zst() {
                    let ptr = self.lower_place_as_ptr(b, dest, lower);
                    if self.lower_rvalue_into(b, rvalue, lower, ptr) {
                        return;
                    }
                    if let Some(value) = self.lower_rvalue(b, rvalue, lower) {
                        b.store(ptr, value);
                    }
                };
            }
        }
    }

    fn lower_terminator<'ir>(
        &mut self,
        mut b: BlockBuilder<'ir, '_>,
        t: &MIRTerminator,
        lower: &mut LIRLower<'ir, '_>,
    ) -> Terminated<()> {
        match t {
            MIRTerminator::Diverge => b.diverge(),
            MIRTerminator::Call { callee, arguments, dest, next, .. } => {
                let next_block = lower.block_map[next];
                let args = arguments
                    .iter()
                    .filter_map(|op| self.lower_operand(&mut b, op, lower))
                    .collect_vec();
                let next = b.target(next_block, vec![]);
                let f = match callee {
                    MIRCallee::Direct(fref) => {
                        let caller_subs = lower.mir.func.subs(self.db);
                        let mut fref = fref.clone();
                        fref.args = fref
                            .args
                            .iter()
                            .map(|ty| {
                                ty.with_substitution(self.db, caller_subs)
                            })
                            .collect();
                        let inst = FuncInst::from_funcref(self.db, fref);
                        self.mir_map.mir_to_lir[&inst]
                    }
                };
                let value = b.call(f, args);
                if let Some(v) = value {
                    if !lower.value_map[dest].is_zst() {
                        let ptr = lower.value_map[dest].ptr();
                        b.store(ptr, v);
                    }
                }
                b.terminate(Terminator::Goto(next))
            }
            MIRTerminator::Return { value, .. } => {
                let value = value
                    .as_ref()
                    .and_then(|op| self.lower_operand(&mut b, op, lower));
                b.ret(value)
            }
            MIRTerminator::Goto { next } => {
                let target = b.target(lower.block_map[next], vec![]);
                b.goto(target)
            }
            MIRTerminator::Branch { cond, then, else_, .. } => {
                let cond = self.lower_operand(&mut b, cond, lower).unwrap();
                let if_true = b.target(lower.block_map[then], vec![]);
                let if_false = b.target(lower.block_map[else_], vec![]);
                b.br(cond, if_true, if_false)
            }
            MIRTerminator::Switch {
                discriminant, branches, default, ..
            } => {
                let on =
                    self.lower_operand(&mut b, discriminant, lower).unwrap();
                let branches = branches
                    .iter()
                    .map(|(n, blk)| {
                        (*n, b.target(lower.block_map[blk], vec![]))
                    })
                    .collect_vec();
                let default = b.target(lower.block_map[default], vec![]);
                b.switch(on, branches, default)
            }
        }
    }

    fn lower_operand<'ir>(
        &mut self,
        b: &mut BlockBuilder<'ir, '_>,
        op: &MIROperand,
        lower: &LIRLower<'ir, '_>,
    ) -> Option<ValueId<'ir>> {
        match op {
            MIROperand::Constant(cst, ..) => match cst {
                MIRConstant::Integer { value, ty } => {
                    let is_ptr = ty.as_ptr(self.db);
                    let layout = layout_of(self.db, *ty);
                    let ty = LIRTy { layout, origin: Some(*ty) };
                    if let Some((_, pointee)) = is_ptr {
                        if *value != 0 {
                            todo!()
                        }
                        let layout = layout_of(self.db, pointee);
                        let pointee = LIRTy { layout, origin: Some(pointee) };
                        Some(b.const_null_ptr(pointee))
                    } else {
                        Some(b.const_int(ty, *value).erase())
                    }
                }
                MIRConstant::Bool(value) => {
                    let layout = LayoutID::int(self.db, IntWidth::I8);
                    let ty =
                        LIRTy { layout, origin: Some(bool_id(self.db).into()) };
                    Some(b.const_int(ty, if *value { 1 } else { 0 }).erase())
                }
                MIRConstant::CString { contents, null_terminated } => {
                    Some(b.strlit(contents, *null_terminated))
                }
            },
            MIROperand::Move(place) | MIROperand::Copy(place) => {
                if !lower.value_map[&place.local].is_zst() {
                    Some(self.lower_place(b, place, lower))
                } else {
                    None
                }
            }
        }
    }

    fn apply_proj<'ir>(
        &mut self,
        b: &mut BlockBuilder<'ir, '_>,
        ptr: ValueId<'ir>,
        ty: TypeRef,
        proj: ProjKind,
        lower: &LIRLower<'ir, '_>,
    ) -> (ValueId<'ir>, TypeRef) {
        match proj {
            ProjKind::Regular(MIRProjection::Deref) => {
                let new_ty = ty
                    .as_ptr(self.db)
                    .unwrap_or_else(|| ty.as_ref(self.db).unwrap())
                    .1;
                let ptr_layout = layout_of(self.db, ty); // layout of the pointer type itself
                let lir_ty = LIRTy { layout: ptr_layout, origin: Some(ty) };
                (b.load(ptr, lir_ty), new_ty)
            }
            ProjKind::Regular(MIRProjection::Field { name, resulting_ty }) => {
                let layout = layout_of(self.db, ty);
                let lir_ty = LIRTy { layout, origin: Some(ty) };
                let src_idx = {
                    let item = struct_item(
                        self.db,
                        ty.as_struct_ref(self.db)
                            .unwrap_or_else(|| {
                                panic!("Type was {}", ty.to_string(self.db))
                            })
                            .def
                            .into(),
                    );
                    let pos = item
                        .fields
                        .iter()
                        .position(|f| f.name == *name)
                        .unwrap();
                    pos as u32
                };
                (b.field_ptr(ptr, lir_ty, src_idx), *resulting_ty)
            }
            ProjKind::Regular(MIRProjection::TupleField {
                index,
                resulting_ty,
            }) => {
                let layout = layout_of(self.db, ty);
                let lir_ty = LIRTy { layout, origin: Some(ty) };
                (b.field_ptr(ptr, lir_ty, *index), *resulting_ty)
            }
            ProjKind::Regular(MIRProjection::Index { index }) => {
                let index = self.lower_operand(b, index, lower).unwrap();
                let elem_ty = ty
                    .as_ptr(self.db)
                    .or_else(|| ty.as_ref(self.db))
                    .unwrap()
                    .1;
                let lir_elem = LIRTy {
                    layout: layout_of(self.db, elem_ty),
                    origin: Some(elem_ty),
                };
                let ptr_ty =
                    LIRTy { layout: layout_of(self.db, ty), origin: Some(ty) };
                let base_ptr = b.load(ptr, ptr_ty);
                (b.index_ptr(base_ptr, lir_elem, index), elem_ty)
            }
            ProjKind::DowncastThen { next, variant } => {
                let _enum_ref = ty.as_enum_ref(self.db).unwrap_or_else(|| {
                    panic!("Expected an enum but got {}", ty.to_string(self.db))
                });
                match next {
                    MIRProjection::Field { .. } => todo!(),
                    MIRProjection::TupleField { index, resulting_ty } => {
                        let layout = layout_of(self.db, ty);
                        let lir_ty = LIRTy { layout, origin: Some(ty) };
                        assert!(lir_ty.is_union(self.db));
                        let vlayout = lir_ty.union_layout(self.db).unwrap();
                        let variant_layout = vlayout.variants[variant as usize];
                        let variant_ty =
                            LIRTy { layout: variant_layout, origin: None };
                        let payload_ptr =
                            b.union_payload_ptr(ptr, lir_ty, variant);
                        let field_ptr =
                            b.field_ptr(payload_ptr, variant_ty, *index);
                        (field_ptr, *resulting_ty)
                    }
                    _ => unreachable!(),
                }
            }
            ProjKind::Regular(MIRProjection::Downcast { .. }) => unreachable!(),
        }
    }

    fn get_proj_kinds<'place>(
        &self,
        place: &'place MIRPlace,
    ) -> Vec<ProjKind<'place>> {
        let mut projs = place.projections.iter().collect_vec();
        projs.reverse();
        let mut res = vec![];
        while let Some(proj) = projs.pop() {
            if let MIRProjection::Downcast { variant } = proj {
                let next = projs.pop().unwrap();
                res.push(ProjKind::DowncastThen {
                    variant: *variant as u32,
                    next,
                });
            } else {
                res.push(ProjKind::Regular(proj));
            }
        }
        res
    }

    fn lower_place_as_ptr<'ir>(
        &mut self,
        b: &mut BlockBuilder<'ir, '_>,
        place: &MIRPlace,
        lower: &LIRLower<'ir, '_>,
    ) -> ValueId<'ir> {
        let slot = &lower.value_map[&place.local];
        if slot.is_zst() {
            unreachable!();
        }
        self.get_proj_kinds(place)
            .into_iter()
            .fold(
                (slot.ptr(), lower.mir.locals[place.local].ty),
                |(ptr, ty), proj| self.apply_proj(b, ptr, ty, proj, lower),
            )
            .0
    }

    fn lower_place<'ir>(
        &mut self,
        b: &mut BlockBuilder<'ir, '_>,
        place: &MIRPlace,
        lower: &LIRLower<'ir, '_>,
    ) -> ValueId<'ir> {
        // Should we load the computed place ptr
        // or eagerly load / extract ?
        // (This should not matter for valid places)
        let layout = layout_of(self.db, place.ty);
        let ty = LIRTy { layout, origin: Some(place.ty) };
        let ptr = self.lower_place_as_ptr(b, place, lower);
        b.load(ptr, ty)
    }

    fn lower_rvalue_into<'ir>(
        &mut self,
        b: &mut BlockBuilder<'ir, '_>,
        rvalue: &MIRRValue,
        lower: &LIRLower<'ir, '_>,
        ptr: ValueId<'ir>,
    ) -> bool {
        match &rvalue.kind {
            MIRRValueKind::Constructor { enum_ref, idx, args, .. } => {
                let variant = *idx as u32;
                let tref = enum_ref.clone().as_type_ref(self.db);
                let layout = layout_of(self.db, tref);
                let ty = LIRTy { layout, origin: Some(tref) };
                let payload_ptr = b.union_payload_ptr(ptr, ty, variant);
                let union_layout = ty.union_layout(self.db).unwrap();
                let variant_layout = union_layout.variants[variant as usize];
                match variant_layout.data(self.db) {
                    LayoutData::Union(_) | LayoutData::Scalar(_) => {
                        // We are expecting a single argument
                        let arg = match args {
                            MIRConstructorArgs::None => {
                                panic!("Expected one argument but got 0")
                            }
                            MIRConstructorArgs::Tuple(ops) => {
                                ops.iter().next().unwrap()
                            }
                            MIRConstructorArgs::Struct(vals) => {
                                vals.values().next().unwrap()
                            }
                        };
                        if let Some(operand) = self.lower_operand(b, arg, lower)
                        {
                            b.store(payload_ptr, operand);
                        }
                    }
                    LayoutData::Aggregate(aggr) => {
                        // We are expecting multiple arguments
                        if aggr.source_field(1).is_none() {
                            // Only a single field !
                            let arg = match args {
                                MIRConstructorArgs::None => {
                                    panic!("Expected one argument but got 0")
                                }
                                MIRConstructorArgs::Tuple(ops) => {
                                    ops.iter().next().unwrap()
                                }
                                MIRConstructorArgs::Struct(vals) => {
                                    vals.values().next().unwrap()
                                }
                            };
                            if let Some(operand) =
                                self.lower_operand(b, arg, lower)
                            {
                                b.store(payload_ptr, operand);
                            }
                        } else {
                            todo!()
                        }
                    }
                    LayoutData::ZeroSized => (),
                }
                b.set_discriminant(ptr, ty, variant);

                true
            }
            _ => false,
        }
    }

    fn lower_rvalue<'ir>(
        &mut self,
        b: &mut BlockBuilder<'ir, '_>,
        rvalue: &MIRRValue,
        lower: &LIRLower<'ir, '_>,
    ) -> Option<ValueId<'ir>> {
        match &rvalue.kind {
            MIRRValueKind::Use(op) => self.lower_operand(b, op, lower),
            MIRRValueKind::Ref(place, _)
            | MIRRValueKind::AddressOf(place, _) => {
                if !lower.value_map[&place.local].is_zst() {
                    Some(self.lower_place_as_ptr(b, place, lower))
                } else {
                    None
                }
            }
            MIRRValueKind::BinOp(op, lhs, rhs) => {
                let lhs = self.lower_operand(b, lhs, lower).unwrap();
                let rhs = self.lower_operand(b, rhs, lower).unwrap();
                let is_signed = true; // todo !!!
                Some(match op {
                    BinaryOperator::Plus => b.arith(
                        if is_signed {
                            ArithBinop::SAdd
                        } else {
                            ArithBinop::UAdd
                        },
                        lhs,
                        rhs,
                    ),
                    BinaryOperator::Minus => b.arith(
                        if is_signed {
                            ArithBinop::SSub
                        } else {
                            ArithBinop::USub
                        },
                        lhs,
                        rhs,
                    ),
                    BinaryOperator::Times => b.arith(
                        if is_signed {
                            ArithBinop::SMul
                        } else {
                            ArithBinop::UMul
                        },
                        lhs,
                        rhs,
                    ),
                    BinaryOperator::Div => b.arith(
                        if is_signed {
                            ArithBinop::SDiv
                        } else {
                            ArithBinop::UDiv
                        },
                        lhs,
                        rhs,
                    ),
                    BinaryOperator::Modulo => b.arith(
                        if is_signed {
                            ArithBinop::SMod
                        } else {
                            ArithBinop::UMod
                        },
                        lhs,
                        rhs,
                    ),
                    BinaryOperator::Eq => b.cmp(CmpBinop::Eq, lhs, rhs),
                    BinaryOperator::Diff => {
                        let eq = b.cmp(CmpBinop::Eq, lhs, rhs);
                        b.not(eq)
                    }
                    BinaryOperator::Lt => b.cmp(
                        if is_signed {
                            CmpBinop::SLessThan
                        } else {
                            CmpBinop::ULessThan
                        },
                        lhs,
                        rhs,
                    ),
                    BinaryOperator::Leq => {
                        let gt = b.cmp(
                            if is_signed {
                                CmpBinop::SLessThan
                            } else {
                                CmpBinop::ULessThan
                            },
                            rhs,
                            lhs,
                        );
                        b.not(gt)
                    }
                    BinaryOperator::Gt => b.cmp(
                        if is_signed {
                            CmpBinop::SLessThan
                        } else {
                            CmpBinop::ULessThan
                        },
                        rhs,
                        lhs,
                    ),
                    BinaryOperator::Geq => {
                        let lt = b.cmp(
                            if is_signed {
                                CmpBinop::SLessThan
                            } else {
                                CmpBinop::ULessThan
                            },
                            lhs,
                            rhs,
                        );
                        b.not(lt)
                    }
                    BinaryOperator::And => todo!(),
                    BinaryOperator::Or => todo!(),
                    BinaryOperator::BitAnd => todo!(),
                    BinaryOperator::BitOr => todo!(),
                    BinaryOperator::BitXor => todo!(),
                })
            }
            MIRRValueKind::UnaryOp(op, operand) => {
                let operand = self.lower_operand(b, operand, lower).unwrap();
                match op {
                    UnaryOperator::Neg => {
                        let ty = b.type_of(operand);
                        let z = b.const_int(ty, 0).erase();
                        Some(b.arith(ArithBinop::SSub, z, operand))
                    }
                    UnaryOperator::LNot => Some(b.not(operand)),
                }
            }
            MIRRValueKind::Discriminant(place) => {
                let layout = layout_of(self.db, place.ty);
                let ty = LIRTy { layout, origin: Some(place.ty) };
                let ptr = self.lower_place_as_ptr(b, place, lower);
                Some(b.get_discriminant(ptr, ty).unwrap().erase())
            }
            MIRRValueKind::Metadata(_) => todo!(),
            MIRRValueKind::SizeOf(type_ref) => {
                let layout = layout_of(self.db, *type_ref);
                let s = layout.size(self.db).bytes();
                Some(
                    b.const_int(
                        LIRTy {
                            layout: LayoutID::int(
                                self.db,
                                self.db.target_width(),
                            ),
                            origin: None,
                        },
                        s as u128,
                    )
                    .erase(),
                )
            }
            MIRRValueKind::Constructor { .. } => {
                unreachable!("Should have been caught by lower_value_into")
            }
            MIRRValueKind::StructLit { struct_ref, fields, .. } => {
                let in_src_order = {
                    let item = struct_item(self.db, struct_ref.def.into());
                    item.fields
                        .iter()
                        .filter_map(|f| {
                            self.lower_operand(b, &fields[&f.name], lower)
                        })
                        .collect_vec()
                };
                let tref = struct_ref.clone().as_type_ref(self.db);
                let layout = layout_of(self.db, tref);
                let ty = LIRTy { layout, origin: Some(tref) };
                Some(b.make_aggregate(ty, in_src_order).erase())
            }
            MIRRValueKind::Tuple(ops, _) => {
                let values = ops
                    .iter()
                    .filter_map(|op| self.lower_operand(b, op, lower))
                    .collect_vec();
                let layout = layout_of(self.db, rvalue.ty);
                let ty = LIRTy { layout, origin: Some(rvalue.ty) };
                Some(b.make_aggregate(ty, values).erase())
            }
            MIRRValueKind::Cast(op, type_ref) => {
                if type_ref.as_ptr(self.db).is_some() {
                    // Should not have to do anything (ptr -> ptr)
                    self.lower_operand(b, op, lower)
                } else {
                    todo!()
                }
            }
        }
    }
}
