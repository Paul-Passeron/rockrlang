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

use std::sync::Arc;

use crate::{
    common::symbols::Symbol,
    parse_tree::expr::BinaryOperator,
    resolved::InterfaceId,
    typecheck::{
        ExprId,
        inference::{
            InferTy, InferenceCtx,
            constraints::{InferenceConstraintKind, MethodConstraint},
            var::InferVar,
        },
    },
};

impl InferenceCtx<'_> {
    pub fn emit_constraint(&mut self, constraint: InferenceConstraintKind) {
        let constraint = Arc::new(self.fresh_constraint(constraint));
        self.ready_push(constraint.id);
        self.register_listeners(&constraint);
        self.all_constraints.insert(constraint.id, constraint);
    }

    pub fn emit_deref_constraint(&mut self, pointee: InferTy) -> InferVar {
        let ptr_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Deref {
            var: ptr_var,
            target: pointee,
        });
        ptr_var
    }

    pub fn emit_indexed_by_constraint(
        &mut self,
        base_ty: InferTy,
        index_ty: InferTy,
    ) -> InferVar {
        let elem_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::IndexedBy {
            elem_var,
            base_ty,
            index_ty,
        });
        elem_var
    }

    pub fn emit_tuple_constraint(
        &mut self,
        tuple_ty: InferTy,
        has_index: u32,
    ) -> InferVar {
        let elem_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Tuple {
            elem_var,
            tuple_ty,
            has_index,
        });
        elem_var
    }

    pub fn emit_struct_field_constraint(
        &mut self,
        struct_ty: InferTy,
        field: Symbol,
    ) -> InferVar {
        let elem_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::StructField {
            elem_var,
            struct_ty,
            field,
        });
        elem_var
    }

    pub fn emit_method_constraint(
        &mut self,
        id: ExprId, // Used to populate the call infos
        ty: InferTy,
        method: Symbol,
        args: Box<[InferTy]>,
        interface_hint: Option<InterfaceId>,
        is_static: bool,
    ) -> InferVar {
        let ret_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Method(MethodConstraint {
            ret_var,
            ty,
            id,
            method,
            args,
            interface_hint,
            is_static,
        }));
        ret_var
    }

    pub fn emit_implements_constraint(
        &mut self,
        ty: InferTy,
        id: InterfaceId,
        args: Box<[InferTy]>,
    ) {
        self.emit_constraint(InferenceConstraintKind::Implements { ty, id, args });
    }

    pub fn emit_binop_constraint(
        &mut self,
        lhs_ty: InferTy,
        rhs_ty: InferTy,
        op: BinaryOperator,
    ) -> InferVar {
        let res_ty = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Binop {
            res_ty,
            lhs_ty,
            rhs_ty,
            op,
        });
        res_ty
    }

    pub fn emit_intlike_constraint(&mut self) -> InferVar {
        let res_ty = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::IntLike { res_ty });
        res_ty
    }

    pub fn emit_binds_like_constraint(
        &mut self,
        like: InferVar,
        ty: InferTy,
    ) -> InferVar {
        let res_ty = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::BindsLike {
            ty: res_ty,
            inner: ty,
            like,
        });
        res_ty
    }

    pub fn emit_is_inner_constraint(&mut self, inner: InferTy, ref_ty: InferTy) {
        self.emit_constraint(InferenceConstraintKind::IsInner { inner, ref_ty });
    }

    pub fn emit_fat_ptr_constraint(&mut self) -> InferVar {
        let fat_ptr_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::FatPtr { fat_ptr_var });
        fat_ptr_var
    }

    pub fn emit_metadata_of_fat_ptr_constraint(
        &mut self,
        fat_ptr_var: InferVar,
    ) -> InferVar {
        let metadata_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::MetadataOfFatPtr {
            fat_ptr_var,
            metadata_var,
        });
        metadata_var
    }
}
