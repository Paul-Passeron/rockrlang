use std::collections::HashMap;

use inkwell::{
    basic_block::BasicBlock,
    types::{BasicMetadataTypeEnum, BasicTypeEnum, FunctionType},
    values::{
        AnyValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue,
        PointerValue, ValueKind,
    },
};
use itertools::Itertools;

use crate::{
    Db,
    compiler::diagnostic::Diag,
    mir::{
        MIR, MIRBlockID, MIRLocalID,
        basic_block::{MIRTerminator, Stmt},
        operand::{
            MIRCallee, MIRConstant, MIROperand, MIRPlace, MIRProjection, MIRRValue,
            MIRRValueKind,
        },
    },
    mir_to_llvm::{LLVMCtx, MyFnType},
    name_resolve::type_expr::struct_item,
    printer::render_diagnostics,
    ril::TypeRef,
    thir::FunctionRef,
    thir_to_mir::FuncInst,
};

pub(super) struct MIRGen<'a, 'b> {
    cg: &'b LLVMCtx<'a, 'b>,
    mir: &'b MIR,

    f: FunctionValue<'a>,
    bb_map: HashMap<MIRBlockID, BasicBlock<'a>>,
    local_map: HashMap<MIRLocalID, PointerValue<'a>>,
}

impl<'a, 'b> MIRGen<'a, 'b> {
    pub fn new(cg: &'b LLVMCtx<'a, 'b>, mir: &'b MIR) -> Self {
        let f = cg.fun_map[&mir.func];
        Self {
            cg,
            mir,
            f,
            bb_map: HashMap::new(),
            local_map: HashMap::new(),
        }
    }

    fn get_block(&self, idx: MIRBlockID) -> BasicBlock<'a> {
        self.bb_map[&idx]
    }

    fn entry(&self) -> BasicBlock<'a> {
        self.get_block(self.mir.entry)
    }

    fn lower_place_as_ptr(&self, place: &MIRPlace) -> PointerValue<'a> {
        let mut llvm_place = self.local(place.local);
        let mut current_ty = self.mir.locals[place.local].ty;
        for proj in &place.projections {
            match proj {
                MIRProjection::Deref => {
                    let new_ty = current_ty;
                    let new_ptr = self
                        .cg
                        .b
                        .build_load(self.ty(new_ty).unwrap(), llvm_place, "")
                        .unwrap();
                    llvm_place = new_ptr.into_pointer_value();
                    current_ty = new_ty
                        .as_ptr(self.cg.db)
                        .unwrap_or_else(|| new_ty.as_ref(self.cg.db).unwrap())
                        .1;
                }
                MIRProjection::Field { name, resulting_ty } => {
                    let pointee_ty = self.ty(current_ty).unwrap().into_struct_type();
                    let sref = current_ty.as_struct_ref(self.cg.db).unwrap();
                    let item = struct_item(self.cg.db, sref.def.into());
                    let index =
                        item.fields
                            .iter()
                            .enumerate()
                            .find_map(|(i, field)| {
                                if field.name == *name { Some(i as u32) } else { None }
                            })
                            .unwrap();
                    llvm_place = self
                        .cg
                        .b
                        .build_struct_gep(pointee_ty, llvm_place, index, "")
                        .unwrap();
                }
                MIRProjection::TupleField {
                    index,
                    resulting_ty,
                } => todo!(),
                MIRProjection::Index { index } => todo!(),
                MIRProjection::Downcast { variant } => todo!(),
            }
        }
        llvm_place
    }

    fn lower_constant(&self, constant: &MIRConstant) -> BasicValueEnum<'a> {
        match constant {
            MIRConstant::Integer { value, ty } => {
                let ty = self.ty(*ty).unwrap().into_int_type();
                ty.const_int(*value as u64, false).into()
            }
            MIRConstant::Bool(value) => {
                self.cg.c.bool_type().const_int(*value as u64, false).into()
            }
            MIRConstant::CString {
                contents,
                null_terminated,
            } => {
                let const_str = self
                    .cg
                    .b
                    .build_global_string_ptr(&unescaper::unescape(&contents).unwrap(), "")
                    .unwrap();
                const_str.as_basic_value_enum()
            }
        }
    }

    fn lower_operand(&self, operand: &MIROperand) -> BasicValueEnum<'a> {
        match operand {
            MIROperand::Constant(mirconstant, _) => self.lower_constant(mirconstant),
            MIROperand::Move(mirplace) | MIROperand::Copy(mirplace) => {
                if let Some(ty) = self.ty(mirplace.ty) {
                    let ptr = self.lower_place_as_ptr(mirplace);
                    self.cg.b.build_load(ty, ptr, "").unwrap()
                } else {
                    unreachable!()
                }
            }
        }
    }

    fn lower_rvalue(&self, rvalue: &MIRRValue) -> BasicValueEnum<'a> {
        let ty = self.ty(rvalue.ty);
        match &rvalue.kind {
            MIRRValueKind::Use(miroperand) => self.lower_operand(miroperand),
            MIRRValueKind::AddressOf(mirplace, _) | MIRRValueKind::Ref(mirplace, _) => {
                self.lower_place_as_ptr(mirplace).as_basic_value_enum()
            }
            MIRRValueKind::BinOp(binary_operator, miroperand, miroperand1) => todo!(),
            MIRRValueKind::UnaryOp(unary_operator, miroperand) => todo!(),
            MIRRValueKind::Discriminant(mirplace) => todo!(),
            MIRRValueKind::Metadata(miroperand) => todo!(),
            MIRRValueKind::SizeOf(type_ref) => todo!(),
            MIRRValueKind::Constructor {
                enum_ref,
                idx,
                args,
                span,
            } => todo!(),
            MIRRValueKind::StructLit {
                struct_ref, fields, ..
            } => {
                let ty = ty.unwrap().into_struct_type();
                let values = struct_item(self.cg.db, struct_ref.def.into())
                    .fields
                    .iter()
                    .map(|field| self.lower_operand(&fields[&field.name]))
                    .collect_vec();
                let mut res = ty.const_zero();
                for (i, val) in values.into_iter().enumerate() {
                    res = self
                        .cg
                        .b
                        .build_insert_value(res, val, i as u32, "")
                        .unwrap()
                        .into_struct_value();
                }
                res.as_basic_value_enum()
            }
            MIRRValueKind::Tuple(miroperands, span) => todo!(),
        }
    }

    fn lower_stmt(&self, stmt: &Stmt) {
        match stmt {
            Stmt::Assign { dest, rvalue } => {
                let place_ptr = self.lower_place_as_ptr(dest);
                let rvalue = self.lower_rvalue(rvalue);
                self.cg.b.build_store(place_ptr, rvalue).unwrap();
            }
        }
    }

    fn lower_terminator(&self, terminator: &MIRTerminator) {
        match terminator {
            MIRTerminator::Diverge => {
                self.cg.b.build_unreachable().unwrap();
            }
            MIRTerminator::Call {
                callee,
                arguments,
                dest,
                next,
                ..
            } => {
                let args = arguments
                    .iter()
                    .map(|arg| self.lower_operand(arg))
                    .map(BasicMetadataValueEnum::from)
                    .collect_vec();

                let llvm_callee = match callee {
                    MIRCallee::Direct(function_ref) => {
                        self.cg.fun_map
                            [&FuncInst::from_funcref(self.cg.db, function_ref.clone())]
                    }
                };

                let call_site_value =
                    self.cg.b.build_direct_call(llvm_callee, &args, "").unwrap();
                match call_site_value.try_as_basic_value() {
                    ValueKind::Basic(value) => {
                        let ptr = self.local_map[dest];
                        self.cg.b.build_store(ptr, value).unwrap();
                    }
                    _ => (),
                }

                self.cg
                    .b
                    .build_unconditional_branch(self.get_block(*next))
                    .unwrap();
            }
            MIRTerminator::Return { value, .. } => {
                if let Some(operand) = value {
                    let lowered: &dyn BasicValue<'a> = &self.lower_operand(operand);
                    self.cg.b.build_return(Some(lowered)).unwrap();
                } else {
                    self.cg.b.build_return(None).unwrap();
                }
            }
            MIRTerminator::Goto { next } => {
                self.cg
                    .b
                    .build_unconditional_branch(self.get_block(*next))
                    .unwrap();
            }
            MIRTerminator::Branch {
                cond, then, else_, ..
            } => {
                let value = self.lower_operand(cond);
                let then_block = self.get_block(*then);
                let else_block = self.get_block(*else_);

                self.cg
                    .b
                    .build_conditional_branch(
                        value.into_int_value(),
                        then_block,
                        else_block,
                    )
                    .unwrap();
            }
            MIRTerminator::Switch {
                discriminant,
                branches,
                default,
                span,
            } => todo!(),
        }
    }

    fn lower_bb(&self, bb: MIRBlockID) {
        self.cg.b.position_at_end(self.get_block(bb));
        let block = &self.mir.blocks[bb];
        block.stmts.iter().for_each(|stmt| self.lower_stmt(stmt));
        self.lower_terminator(&block.terminator);
    }

    pub fn lower(mut self) {
        self.build_bb_map();
        self.cg.b.position_at_end(self.entry());
        self.build_local_map_and_allocate();
        self.handle_params();
        self.mir.blocks.keys().for_each(|bb| self.lower_bb(bb));
    }

    fn local(&self, idx: MIRLocalID) -> PointerValue<'a> {
        self.local_map[&idx]
    }

    fn handle_params(&mut self) {
        for (i, param) in self.mir.parameters.iter().enumerate() {
            let llvm_local = self.local(*param);
            let value = self.f.get_nth_param(i as u32).unwrap_or_else(|| {
                panic!(
                    "In function {} trying to get argument {i}\n{}",
                    self.mir.func.fdef(self.cg.db).sig_to_string(self.cg.db),
                    self.f.print_to_string().to_string()
                )
            });
            self.cg.b.build_store(llvm_local, value).unwrap();
        }
    }

    fn ty(&self, ty: TypeRef) -> Option<BasicTypeEnum<'a>> {
        ty.as_type_id()
            .unwrap()
            .interned()
            .as_llvm(self.cg.db, self.cg.c)
            .try_into()
            .ok()
    }

    fn build_local_map_and_allocate(&mut self) {
        for (idx, local) in &self.mir.locals {
            if let Some(ty) = self.ty(local.ty) {
                let name = local
                    .name
                    .map_or("".into(), |symbol| symbol.to_string(self.cg.db));
                let llvm_local = self.cg.b.build_alloca(ty, name.as_str()).unwrap();
                self.local_map.insert(idx, llvm_local);
            }
        }
    }

    fn create_block_for(&self, idx: MIRBlockID) -> BasicBlock<'a> {
        let block = &self.mir.blocks[idx];
        let llvm_block = self.cg.c.append_basic_block(
            self.f,
            block.name.as_ref().map(String::as_str).unwrap_or(""),
        );
        llvm_block
    }

    fn build_bb_map(&mut self) {
        let entry = self.mir.entry;
        let llvm_entry = self.create_block_for(entry);
        self.bb_map.insert(entry, llvm_entry);
        for idx in self.mir.blocks.keys() {
            if idx != entry {
                let llvm_block = self.create_block_for(idx);
                self.bb_map.insert(idx, llvm_block);
            }
        }
    }
}
