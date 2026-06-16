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

use std::{collections::HashMap, sync::Arc};

use itertools::{Either, Itertools};

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    hir::Mutability,
    mir::{
        MIR, MIRBlockID, MIRLocal, MIRLocalID, SyntacticSource,
        basic_block::{MIRTerminator, Stmt},
        builder::MIRBuilder,
        operand::{
            MIRCallee, MIRConstant, MIRConstructorArgs, MIROperand, MIRPlace,
            MIRProjection, MIRRValue, MIRRValueKind, UnaryOperator,
        },
    },
    ril::{
        BuiltinTypeId, FunctionId, TypeDefId, TypeId, TypeRef, bool_id, char_id, int_id,
        never_id, str_def, usize_id, void_id,
    },
    thir::{
        self, EnumRef, ExprId, ExprKind, FunctionRef, PlaceBase, PlaceId, Projection,
        ScopeId, StructRef, Thir, ThirConstructorArgs, ThirExprWithSetup,
        ThirMatchBranch,
        stmt::{StmtKind, ThirStmt},
        thir_body,
    },
    unused,
};

pub struct ThirToMIR<'a> {
    db: &'a dyn Db,
    builder: MIRBuilder<'a>,
    thir: &'a Thir,
    subs: &'a [TypeRef],

    local_map: HashMap<thir::LocalId, MIRLocalID>,
    params: Vec<MIRLocalID>,
    loop_begins: HashMap<ScopeId, MIRBlockID>,
    loop_ends: HashMap<ScopeId, MIRBlockID>,
}

#[salsa::interned]
pub struct MIRKey {
    fdef: FunctionId,

    #[returns(ref)]
    subs: Vec<TypeRef>,
}

/// Wrapper to send MIR safely between threads as it is supposed to be read-only
#[derive(Clone, PartialEq, Eq)]
struct _MIRWrapper(Arc<MIR>);
unsafe impl Sync for _MIRWrapper {}
unsafe impl Send for _MIRWrapper {}

#[salsa::tracked]
fn _mir<'db>(db: &'db dyn Db, key: MIRKey<'db>) -> _MIRWrapper {
    let Some(thir) = thir_body(db, key.fdef(db)) else {
        panic!("attempted to lower extern function to MIR")
    };
    _MIRWrapper(Arc::new(
        ThirToMIR::new(db, thir.as_ref(), key.subs(db)).lower(),
    ))
}

pub fn mir(db: &dyn Db, fdef: FunctionId, subs: Vec<TypeRef>) -> Arc<MIR> {
    _mir(db, MIRKey::new(db, fdef, subs)).0
}

impl<'a> ThirToMIR<'a> {
    pub fn new(db: &'a dyn Db, thir: &'a Thir, subs: &'a [TypeRef]) -> Self {
        Self {
            db,
            builder: MIRBuilder::new(db, Some("entry".into())),
            thir,
            subs,
            local_map: HashMap::new(),
            params: Vec::new(),
            loop_begins: HashMap::new(),
            loop_ends: HashMap::new(),
        }
    }

    pub fn is_valid_ty(&self, ty: TypeRef) -> bool {
        match ty {
            TypeRef::Concrete(type_id) => type_id
                .args(self.db)
                .into_iter()
                .all(|ty| self.is_valid_ty(ty)),
            TypeRef::Param(_) => false,
            TypeRef::Associated(_) => todo!(),
            _ => false,
        }
    }

    /// Returns the substituted form of the type
    pub fn ty(&self, ty: TypeRef) -> TypeRef {
        let res = match ty {
            TypeRef::Concrete(type_id) => TypeRef::Concrete(TypeId::new(
                self.db,
                type_id.def(self.db),
                type_id
                    .args(self.db)
                    .into_iter()
                    .map(|ty| self.ty(ty))
                    .collect(),
            )),
            TypeRef::Param(id) => self.subs[id.0],
            TypeRef::Zelf => {
                let unsub = self
                    .thir
                    .id
                    .parent(self.db)
                    .get_canonical_zelf(self.db)
                    .unwrap_or(TypeRef::Error);
                if matches!(unsub, TypeRef::Zelf) {
                    // Do not recurse infinitely, bail early
                    TypeRef::Error
                } else {
                    self.ty(unsub)
                }
            }
            TypeRef::Associated(_) => todo!(),
            _ => ty,
        };
        if !self.is_valid_ty(res) {
            panic!("Not a valid ty")
        }
        res
    }

    fn build_thir_locals(&mut self) {
        for (thir_id, local) in self.thir.locals.iter() {
            let ty = self.ty(local.ty);
            let mut mir = MIRLocal::new(ty, local.mutability, local.span);
            if let Some(src) = &local.source {
                mir = mir.with_name(src.1).with_thir_src(thir_id).with_syn_src(
                    SyntacticSource {
                        span: local.span,
                        id: src.0,
                    },
                )
            }
            let mir_id = self.builder.new_local(mir);

            self.local_map.insert(thir_id, mir_id);
        }
    }

    fn build_arguments(&mut self) {
        for param in &self.thir.params {
            let mir_id = *self.local_map.get(param).expect("Expected all local thir ids to be found in self.local_map. Maybe try calling `build_thir_locals`.");
            self.params.push(mir_id);
        }
        self.builder.set_parameters(self.params.clone()).unwrap();
    }

    fn check_substitution(&self) {
        for ty in self.subs {
            if !self.is_valid_ty(*ty) {
                panic!(
                    "Expecting a valid type in substitution but got {}",
                    ty.to_string(self.db)
                )
            }
        }
    }

    fn build_stmts(&mut self, stmts: &[ThirStmt]) {
        for stmt in stmts {
            self.build_stmt(stmt);
        }
    }

    fn synthetic_local(&self, ty: TypeRef, span: Span) -> MIRLocalID {
        let local = MIRLocal::new(ty, Mutability::Const, span);
        self.builder.new_local(local)
    }

    fn place_of_local(&self, local: MIRLocalID) -> MIRPlace {
        let ty = self.builder.locals[local].ty;
        MIRPlace {
            local,
            projections: vec![],
            ty,
        }
    }

    fn synthetic_place(&self, ty: TypeRef, span: Span) -> MIRPlace {
        self.place_of_local(self.synthetic_local(ty, span))
    }

    fn goto(&mut self, next: MIRBlockID, span: Span) {
        self.builder
            .terminate(MIRTerminator::Goto { next, span })
            .unwrap()
    }

    fn build_stmt(&mut self, stmt: &ThirStmt) {
        match &stmt.kind {
            // For the moment, we do not have to care about destructors or anything so
            // we just:
            StmtKind::Block { stmts, .. } => self.build_stmts(stmts),
            StmtKind::If {
                cond, then, else_, ..
            } => {
                self.build_if_stmt(
                    cond,
                    then,
                    else_.as_ref().map(|x| x.as_slice()),
                    stmt.span,
                );
            }
            StmtKind::While { scope, cond, body } => {
                self.build_while_stmt(*scope, cond, body, stmt.span)
            }
            StmtKind::Let { local, init } => {
                let mir_local = self.local_map[local];
                let dest = self.place_of_local(mir_local);
                let rvalue = self.build_rvalue(*init);
                self.assign(dest, rvalue);
            }
            StmtKind::Assign { place, rhs } => {
                let dest = self.build_place(*place);
                let rvalue = self.build_rvalue(*rhs);
                self.assign(dest, rvalue);
            }
            StmtKind::Return(expr) => {
                let value = expr.map(|expr| self.build_operand(expr));
                self.build_ret(value, stmt.span);
                self.switch_to(self.builder.new_block(Some("dead".into())));
            }
            StmtKind::Break(scope_id) => {
                let bb = self.loop_ends[scope_id];
                self.goto(bb, stmt.span);
            }
            StmtKind::Continue(scope_id) => {
                let bb = self.loop_begins[scope_id];
                self.goto(bb, stmt.span);
            }
            StmtKind::Match {
                scrutinee,
                branches,
            } => self.build_match_stmt(scrutinee, branches),
            StmtKind::Expr(idx) => {
                self.build_operand(*idx);
            }
            StmtKind::Error => panic!("Can only produce MIR of error-less THIR"),
        }
    }

    fn build_match_stmt(
        &mut self,
        scrutinee: &ThirExprWithSetup,
        branches: &[ThirMatchBranch],
    ) {
        let _mir_scrut = self.build_expr_with_setup(scrutinee);
        unused!(branches);
        todo!()
    }

    fn build_place_base(&self, base: PlaceBase) -> MIRLocalID {
        match base {
            PlaceBase::Local(local) => self.local_map[&local],
        }
    }

    fn build_projection(&mut self, projection: Projection) -> MIRProjection {
        match projection {
            Projection::Deref => MIRProjection::Deref,
            Projection::Field(name, ty) => {
                let resulting_ty = self.ty(ty);
                MIRProjection::Field { name, resulting_ty }
            }
            Projection::TupleField(index, ty) => {
                let resulting_ty = self.ty(ty);
                MIRProjection::TupleField {
                    index,
                    resulting_ty,
                }
            }
            Projection::Index(expr) => {
                let index = self.build_operand(expr);
                MIRProjection::Index { index }
            }
        }
    }

    fn build_projections(&mut self, projections: &[Projection]) -> Vec<MIRProjection> {
        projections
            .iter()
            .map(|projection| self.build_projection(*projection))
            .collect()
    }

    fn build_place(&mut self, place: PlaceId) -> MIRPlace {
        let thir_place = &self.thir.places[place];
        let local = self.build_place_base(thir_place.base);
        let projections = self.build_projections(&thir_place.projections);
        let ty = self.ty(thir_place.ty);

        MIRPlace {
            local,
            projections,
            ty,
        }
    }

    fn move_or_copy(&self, place: MIRPlace) -> MIROperand {
        if place.ty.is_copy(self.db) { place.into_copy() } else { place.into_move() }
    }

    fn struct_ref(&mut self, struct_ref: &StructRef) -> StructRef {
        StructRef {
            def: struct_ref.def,
            args: struct_ref.args.iter().map(|ty| self.ty(*ty)).collect_vec(),
        }
    }

    fn enum_ref(&mut self, enum_ref: &EnumRef) -> EnumRef {
        EnumRef {
            def: enum_ref.def,
            args: enum_ref.args.iter().map(|ty| self.ty(*ty)).collect_vec(),
        }
    }

    fn get_str_struct_ref(&self) -> StructRef {
        let TypeDefId::Struct(str_def) = str_def(self.db) else {
            unreachable!()
        };
        StructRef {
            def: str_def,
            args: vec![],
        }
    }

    fn build_strlit(&mut self, strlit: &str) -> MIRRValueKind {
        let data = MIRConstant::CString {
            contents: strlit.into(),
            null_terminated: false,
        };
        let len = MIRConstant::Integer {
            value: strlit.len() as i128,
            ty: usize_id(self.db).into(),
        };

        MIRRValueKind::StructLit {
            struct_ref: self.get_str_struct_ref(),
            fields: HashMap::from_iter(vec![
                (Symbol::new(self.db, "data"), data.into()),
                (Symbol::new(self.db, "len"), len.into()),
            ]),
        }
    }

    fn build_rvalue(&mut self, expr: ExprId) -> MIRRValue {
        let thir_expr = &self.thir.exprs[expr];
        let ty = self.ty(thir_expr.ty);
        let span = thir_expr.span;
        let kind = match &thir_expr.kind {
            ExprKind::StrLit(lit) => self.build_strlit(&lit.interned().contents(self.db)),
            ExprKind::StructLit {
                struct_def,
                fields: thir_fields,
            } => {
                let struct_ref = self.struct_ref(struct_def);
                let fields = self.build_fields(thir_fields);
                MIRRValueKind::StructLit { struct_ref, fields }
            }
            ExprKind::Constructor {
                enum_def,
                idx,
                args,
            } => MIRRValueKind::Constructor {
                enum_ref: self.enum_ref(enum_def),
                idx: *idx,
                args: self.build_constructor_args(args),
            },
            ExprKind::Use(place) => {
                let place = self.build_place(*place);
                MIRRValueKind::Use(self.move_or_copy(place))
            }
            ExprKind::AddressOf { place, mutability } => {
                MIRRValueKind::AddressOf(self.build_place(*place), *mutability)
            }
            ExprKind::Ref { place, mutability } => {
                MIRRValueKind::Ref(self.build_place(*place), *mutability)
            }
            ExprKind::Call { called, args } => {
                let local = self.build_call(called.clone(), args, ty, span);
                MIRRValueKind::Use(self.place_of_local(local).into_move())
            }
            ExprKind::BinOp { op, lhs, rhs } => MIRRValueKind::BinOp(
                *op,
                self.build_operand(*lhs),
                self.build_operand(*rhs),
            ),
            ExprKind::Neg(expr) => {
                let operand = self.build_operand(*expr);
                MIRRValueKind::UnaryOp(UnaryOperator::Neg, operand)
            }
            ExprKind::Not(expr) => {
                let operand = self.build_operand(*expr);
                MIRRValueKind::UnaryOp(UnaryOperator::LNot, operand)
            }
            ExprKind::Tuple(exprs) => MIRRValueKind::Tuple(
                exprs
                    .iter()
                    .map(|expr| self.build_operand(*expr))
                    .collect_vec(),
            ),
            ExprKind::SizeOf(ty) => MIRRValueKind::SizeOf(self.ty(*ty)),
            ExprKind::Metadata(expr) => {
                MIRRValueKind::Metadata(self.build_operand(*expr))
            }
            _ if let Some(cst) = self.build_expr_as_constant(expr) => {
                MIRRValueKind::Use(cst.into())
            }
            ExprKind::SliceLit(_) => todo!(),
            ExprKind::Error => panic!("Can only produce MIR of error-less THIR"),
            _ => {
                unreachable!("Unhandled expression at {}", span.start().loc_info(self.db))
            }
        };
        MIRRValue { kind, ty, span }
    }

    fn build_fields(
        &mut self,
        fields: &[(Symbol, ExprId)],
    ) -> HashMap<Symbol, MIROperand> {
        HashMap::from_iter(fields.iter().map(|(name, expr)| {
            let operand = self.build_operand(*expr);
            (*name, operand)
        }))
    }

    fn build_constructor_args(
        &mut self,
        args: &ThirConstructorArgs<ExprId>,
    ) -> MIRConstructorArgs {
        match args {
            ThirConstructorArgs::Tuple(exprs) => MIRConstructorArgs::Tuple(
                exprs
                    .iter()
                    .map(|expr| self.build_operand(*expr))
                    .collect_vec(),
            ),
            ThirConstructorArgs::Struct(fields) => {
                MIRConstructorArgs::Struct(self.build_fields(fields))
            }
            ThirConstructorArgs::None => MIRConstructorArgs::None,
        }
    }

    fn build_call(
        &mut self,
        called: FunctionRef,
        args: &[ExprId],
        ret_ty: TypeRef,
        span: Span,
    ) -> MIRLocalID {
        let callee = MIRCallee::Direct(called);
        let args = args
            .iter()
            .map(|arg| self.build_operand(*arg))
            .collect_vec();
        let next_bb = self.builder.new_block(None);
        let local = self.synthetic_local(ret_ty, span);
        self.build_terminator(MIRTerminator::Call {
            callee,
            arguments: args,
            dest: local,
            next: next_bb,
            span,
        });
        self.switch_to(next_bb);
        if ret_ty == never_id(self.db).into() {
            self.build_terminator(MIRTerminator::Diverge);
            let unreachable_bb = self.builder.new_block(Some("dead".into()));
            self.switch_to(unreachable_bb);
        }
        local
    }

    fn branch(
        &mut self,
        cond: MIROperand,
        then_bb: MIRBlockID,
        else_bb: MIRBlockID,
        span: Span,
    ) {
        self.build_terminator(MIRTerminator::Branch {
            cond,
            then: then_bb,
            else_: else_bb,
            span,
        });
    }

    fn switch_to(&mut self, block: MIRBlockID) {
        self.builder.switch_to_block(block).unwrap();
    }

    fn build_while_stmt(
        &mut self,
        scope: ScopeId,
        cond: &ThirExprWithSetup,
        body: &[ThirStmt],
        span: Span,
    ) {
        let cond_bb = self.builder.new_block(Some("while-condition".into()));
        let body_bb = self.builder.new_block(Some("while-body".into()));
        let merge_bb = self.builder.new_block(Some("while-merge".into()));

        self.loop_begins.insert(scope, cond_bb);
        self.loop_ends.insert(scope, merge_bb);

        self.goto(cond_bb, span);

        self.switch_to(cond_bb);
        let cond = self.build_expr_with_setup(cond);
        self.branch(cond, body_bb, merge_bb, span);

        self.switch_to(body_bb);
        self.build_stmts(body);

        if !self.current_block_is_terminated() {
            self.goto(cond_bb, span);
        }

        self.switch_to(merge_bb);
    }

    fn build_if_stmt(
        &mut self,
        cond: &ThirExprWithSetup,
        then: &[ThirStmt],
        else_: Option<&[ThirStmt]>,
        span: Span,
    ) {
        let cond = self.build_expr_with_setup(cond);

        let then_bb = self.builder.new_block(Some("if-then".into()));
        let merge_bb = self.builder.new_block(Some("if-merge".into()));

        let end_span = span.end().span(span.end());

        if let Some(else_stmts) = else_ {
            let else_bb = self.builder.new_block(Some("if-else".into()));

            self.branch(cond, then_bb, else_bb, span);

            self.switch_to(else_bb);
            self.build_stmts(else_stmts);
            if !self.current_block_is_terminated() {
                self.goto(merge_bb, end_span);
            }
        } else {
            self.branch(cond, then_bb, merge_bb, span);
        }

        self.switch_to(then_bb);
        self.build_stmts(then);
        if !self.current_block_is_terminated() {
            self.goto(merge_bb, end_span);
        }

        self.switch_to(merge_bb);
    }

    fn build_expr_with_setup(&mut self, expr: &ThirExprWithSetup) -> MIROperand {
        self.build_stmts(&expr.stmts);
        self.build_operand(expr.expr)
    }

    fn build_rvalue_or_place(&mut self, expr: ExprId) -> Either<MIRRValue, MIRPlace> {
        let thir_expr = &self.thir.exprs[expr];
        match &thir_expr.kind {
            ExprKind::Use(place) => Either::Right(self.build_place(*place)),
            ExprKind::Call { called, args } => {
                let ret_ty = self.ty(thir_expr.ty);
                let local = self.build_call(called.clone(), args, ret_ty, thir_expr.span);
                Either::Right(self.place_of_local(local))
            }
            _ => Either::Left(self.build_rvalue(expr)),
        }
    }

    fn assign(&mut self, dest: MIRPlace, rvalue: MIRRValue) {
        self.builder.emit(Stmt::Assign { dest, rvalue });
    }

    fn build_expr_as_constant(&mut self, expr: ExprId) -> Option<MIRConstant> {
        let thir_expr = &self.thir.exprs[expr];
        match &thir_expr.kind {
            ExprKind::IntLit(value) => {
                let ty = self.ty(thir_expr.ty);
                Some(MIRConstant::Integer {
                    value: *value as i128,
                    ty,
                })
            }
            // TODO: Handle unicode one day
            ExprKind::Charlit(lit) => Some(MIRConstant::Integer {
                value: *lit as u8 as i128,
                ty: char_id(self.db).into(),
            }),
            ExprKind::CStrLit(str_lit) => Some(MIRConstant::CString {
                contents: str_lit.interned().contents(self.db),
                null_terminated: true,
            }),
            ExprKind::BoolLit(value) => Some(MIRConstant::Bool(*value)),
            _ => None,
        }
    }

    fn build_operand(&mut self, expr: ExprId) -> MIROperand {
        if let Some(cst) = self.build_expr_as_constant(expr) {
            return MIROperand::Constant(cst);
        }

        match self.build_rvalue_or_place(expr) {
            Either::Left(rvalue) => {
                let ty = rvalue.ty;
                let place = self.synthetic_place(ty, rvalue.span);
                self.assign(place.clone(), rvalue);
                place.into_move()
            }
            Either::Right(place) => self.move_or_copy(place),
        }
    }

    fn build_terminator(&mut self, terminator: MIRTerminator) {
        self.builder.terminate(terminator).unwrap();
    }

    fn build_ret(&mut self, value: Option<MIROperand>, span: Span) {
        self.build_terminator(MIRTerminator::Return { value, span })
    }

    fn build_void_ret(&mut self, span: Span) {
        self.build_ret(None, span);
    }

    fn get_ret_ty(&self) -> TypeRef {
        self.ty(self.thir.get_ret_ty(self.db))
    }

    fn current_block_is_terminated(&self) -> bool {
        self.builder.is_terminated(self.builder.current_block())
    }

    fn build_void_ret_if_needed(&mut self) {
        if self.current_block_is_terminated() {
            return;
        }
        if self.get_ret_ty() != void_id(self.db).into() {
            return;
        }

        let loc = self.thir.body_span(self.db).end();
        self.build_void_ret(loc.span(loc));
    }

    pub fn lower(mut self) -> MIR {
        // Only during debug ?
        self.check_substitution();

        self.build_thir_locals();
        self.build_arguments();

        self.build_stmts(&self.thir.root);

        self.build_void_ret_if_needed();

        if !self.current_block_is_terminated() {
            self.build_terminator(MIRTerminator::Diverge);
        }

        self.builder
            .finalize()
            .expect("Something went wrong finalizing the builder")
    }
}

/// Cheking if a type can be copied. This will be delegated to an interface
/// check.

impl TypeRef {
    pub fn is_copy(self, db: &dyn Db) -> bool {
        match self {
            TypeRef::Concrete(type_id) => type_id.is_copy(db),
            _ => false, // TODO
        }
    }
}

impl TypeId {
    pub fn is_copy(self, db: &dyn Db) -> bool {
        if self == int_id(db) {
            return true;
        }
        if self == bool_id(db) {
            return true;
        }
        if self == char_id(db) {
            return true;
        }
        if self == usize_id(db) {
            return true;
        }
        let def = self.def(db);
        if def == BuiltinTypeId::ref_(db).into() {
            return true;
        }
        if def == BuiltinTypeId::mut_ptr(db).into() {
            return true;
        }
        if def == BuiltinTypeId::ptr(db).into() {
            return true;
        }

        false
    }
}
