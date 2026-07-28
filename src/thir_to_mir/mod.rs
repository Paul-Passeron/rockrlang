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

use std::{
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
};

use itertools::{Either, Itertools};
use salsa::Accumulator;

use crate::{
    Db,
    check::fundef::concretize_fid,
    common::{location::Span, symbols::Symbol},
    compiler::{Workspace, diagnostic::Diag, timing::{Counts, Phase, timed}, workspace_packages},
    hir::{Mutability, function_ast, signature::get_sig_of_function},
    mir::{
        MIRBlockID, MIRLocal, MIRLocalID, Mir, SyntacticSource,
        basic_block::{MIRTerminator, Stmt},
        builder::MIRBuilder,
        operand::{
            MIRCallee, MIRConstant, MIRConstructorArgs, MIROperand, MIRPlace,
            MIRProjection, MIRRValue, MIRRValueKind, UnaryOperator,
        },
    },
    name_resolve::{
        core_package, file_module_id, std_package, type_expr::get_templates_of_fun,
    },
    resolved::{
        BuiltinTypeKind, FunctionId, ScopeOwnerId, TypeDefId, TypeId, TypeRef, char_id,
        never_id, str_def, str_id, void_id,
    },
    thir::{
        self, EnumRef, ExprId, ExprKind, FunctionRef, PlaceBase, PlaceId, Projection,
        ScopeId, StructRef, Thir, ThirConstructorArgs, ThirExprWithSetup,
        ThirMatchBranch, ThirStructField,
        stmt::{StmtKind, ThirStmt},
        thir_body,
    },
    thir_to_mir::lower_match::MatchLowerer,
};

pub mod decision_tree;
pub mod lower_match;

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
    pub fdef: FunctionId,

    pub subs: Vec<TypeRef>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncInst(pub salsa::Id);

impl<'db> From<MIRKey<'db>> for FuncInst {
    fn from(value: MIRKey<'db>) -> Self {
        Self(value.0)
    }
}

impl FuncInst {
    pub fn interned<'db>(self) -> MIRKey<'db> {
        MIRKey(self.0, PhantomData)
    }

    pub fn fdef(self, db: &dyn Db) -> FunctionId {
        *self.interned().fdef(db)
    }

    pub fn subs(self, db: &dyn Db) -> &[TypeRef] {
        self.interned().subs(db)
    }

    pub fn from_funcref(db: &dyn Db, fref: FunctionRef) -> Self {
        assert_eq!(get_templates_of_fun(db, fref.id.interned()).len(), fref.args.len());
        MIRKey::new(db, fref.id, fref.args).into()
    }

    pub fn ret_ty(self, db: &dyn Db) -> TypeRef {
        let sig = get_sig_of_function(db, self.fdef(db).interned());
        let ret_ty = match sig.ret {
            TypeRef::Zelf => {
                self.fdef(db).parent(db).get_canonical_zelf(db).unwrap_or(TypeRef::Error)
            }
            ret => ret,
        };
        ret_ty.with_substitution(db, self.subs(db))
    }

    pub fn params(self, db: &dyn Db) -> Vec<(Symbol, TypeRef)> {
        let sig = get_sig_of_function(db, self.fdef(db).interned());
        let fdef = self.fdef(db);
        let zelf = fdef.parent(db).get_canonical_zelf(db);
        let receiver = sig.zelf.map(|arg| {
            (
                Symbol::new(db, "self"),
                arg.as_type_ref_for(db, zelf.unwrap_or(TypeRef::Error))
                    .with_substitution(db, self.subs(db)),
            )
        });
        receiver
            .into_iter()
            .chain(sig.args.iter().map(|(symb, ty)| {
                (
                    *symb,
                    match ty.with_substitution(db, self.subs(db)) {
                        TypeRef::Zelf => zelf.expect("We should have a `Self` type here"),
                        ty => ty,
                    },
                )
            }))
            .collect()
    }
}

#[salsa::tracked]
impl FuncInst {
    pub fn is_variadic(self, db: &dyn Db) -> bool {
        match function_ast(db, self.fdef(db).interned()).inner(db) {
            crate::hir::FunctionLikeAst::ExternDef(_, variadic) => *variadic,
            _ => false,
        }
    }

    pub fn is_main(self, db: &dyn Db) -> bool {
        let packages = workspace_packages(db, Workspace::get(db));
        let mut main_pkg = None;
        for pkg in packages {
            if Some(*pkg) == std_package(db) {
                continue;
            }
            if *pkg == core_package(db) {
                continue;
            }
            assert!(main_pkg.is_none(), "TODO: handle multiple packages for main");

            main_pkg = Some(*pkg);
        }
        let main_pkg = main_pkg.unwrap();
        let file_module = file_module_id(db, *main_pkg.root(db), None, main_pkg);
        let f_id = FunctionId::new(
            db,
            Symbol::new(db, "main"),
            ScopeOwnerId::Module(file_module),
        );
        if self.fdef(db) != f_id {
            return false;
        }
        assert!(self.subs(db).is_empty(), "Generic main function !");

        true
    }
}

#[salsa::tracked]
pub fn _mir<'db>(db: &'db dyn Db, key: MIRKey<'db>) -> Mir {
    timed(
        db,
        Phase::Monomorphization,
        || {
            let Some(thir) = thir_body(db, *key.fdef(db)) else {
                panic!("attempted to lower extern function to MIR")
            };
            ThirToMIR::new(db, thir, key.subs(db)).lower()
        },
        |mir| Counts {
            functions: 1,
            stmts: Some(mir.blocks.iter().map(|(_, b)| b.stmts.len() + 1).sum()),
            exprs: None,
        },
    )
}

pub fn mir(db: &dyn Db, fdef: FunctionId, subs: Vec<TypeRef>) -> &Mir {
    _mir(db, MIRKey::new(db, fdef, subs))
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

    pub fn is_concrete(&self, ty: TypeRef) -> bool {
        match ty {
            TypeRef::Concrete(type_id) => {
                type_id.args(self.db).iter().all(|ty| self.is_concrete(*ty))
            }
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
                type_id.args(self.db).iter().map(|ty| self.ty(*ty)).collect(),
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
        if !self.is_concrete(res) {
            // panic!("Not a valid ty: {}", res.to_string(self.db))
        }
        res
    }

    fn build_thir_locals(&mut self) {
        for (thir_id, local) in self.thir.locals.iter() {
            let ty = self.ty(local.ty);
            let mut mir = MIRLocal::new(ty, local.mutability, local.span);
            if let Some(src) = &local.source {
                mir = mir
                    .with_name(src.1)
                    .with_thir_src(thir_id)
                    .with_syn_src(SyntacticSource { span: local.span, id: src.0 });
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
        self.builder
            .set_parameters(self.params.clone())
            .expect("Cannot set parameters multiple times");
    }

    fn check_substitution(&self) {
        for ty in self.subs {
            assert!(
                self.is_concrete(*ty),
                "Expecting a valid type in substitution but got {}",
                ty.to_string(self.db)
            );
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

    fn place_of_local(&self, local: MIRLocalID, span: Span) -> MIRPlace {
        let ty = self.builder.locals[local].ty;
        MIRPlace { local, projections: vec![], ty, span }
    }

    fn synthetic_place(&self, ty: TypeRef, span: Span) -> MIRPlace {
        self.place_of_local(self.synthetic_local(ty, span), span)
    }

    fn goto(&mut self, next: MIRBlockID) {
        self.builder
            .terminate(MIRTerminator::Goto { next })
            .expect("Cannot goto to terminated block");
    }

    fn build_stmt(&mut self, stmt: &ThirStmt) {
        match &stmt.kind {
            // For the moment, we do not have to care about destructors or
            // anything so we just:
            StmtKind::Block { stmts, .. } => self.build_stmts(stmts),
            StmtKind::If { cond, then, else_, .. } => {
                self.build_if_stmt(
                    cond,
                    then,
                    else_.as_ref().map(Vec::as_slice),
                    stmt.span,
                );
            }
            StmtKind::While { scope, cond, body } => {
                self.build_while_stmt(*scope, cond, body, stmt.span);
            }
            StmtKind::Let { local, init } => {
                let mir_local = self.local_map[local];
                let dest =
                    self.place_of_local(mir_local, self.builder.locals[mir_local].span);
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
                self.goto(bb);
            }
            StmtKind::Continue(scope_id) => {
                let bb = self.loop_begins[scope_id];
                self.goto(bb);
            }
            StmtKind::Match { scrutinee, branches } => {
                self.build_match_stmt(scrutinee, branches);
            }
            StmtKind::Expr(idx) => {
                let rval = self.build_rvalue(*idx);
                self.spill_rvalue_if_needed(rval, stmt.span);
            }
            StmtKind::Error => {} /* Don't crash so the user can still see errors
                                   * afterwards */
        }
    }

    fn spill_rvalue_if_needed(&mut self, rval: MIRRValue, span: Span) {
        match &rval.kind {
            MIRRValueKind::Use(op) => match op {
                MIROperand::Move(place) | MIROperand::Copy(place)
                    if !place.projections.is_empty() =>
                {
                    self.assign(self.synthetic_place(rval.ty, span), rval);
                }
                _ => (),
            },
            _ => self.assign(self.synthetic_place(rval.ty, span), rval),
        }
    }

    fn spill_operand(&mut self, operand: MIROperand, span: Span) -> MIRPlace {
        if let MIROperand::Move(place) = operand {
            return place;
        }
        let ty = self.ty(operand.ty(self.db));
        let as_rvalue = MIRRValue { kind: MIRRValueKind::Use(operand), ty, span };
        let place = self.synthetic_place(ty, span);
        self.builder.emit(Stmt::Assign { dest: place.clone(), rvalue: as_rvalue });
        place
    }

    fn build_match_stmt(
        &mut self,
        scrutinee: &ThirExprWithSetup,
        branches: &[ThirMatchBranch],
    ) {
        let scrut_op = self.build_expr_with_setup(scrutinee);
        let span = self.thir.exprs[scrutinee.expr].span;
        let scrut_place = self.spill_operand(scrut_op, span);
        let merge_bb = self.builder.new_block(Some("switch-merge".into()));
        MatchLowerer::new(self, scrut_place, merge_bb).lower(branches);
        self.switch_to(merge_bb);
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
                MIRProjection::TupleField { index, resulting_ty }
            }
            Projection::Index(expr) => {
                let index = self.build_operand(expr);
                MIRProjection::Index { index }
            }
        }
    }

    fn build_projections(&mut self, projections: &[Projection]) -> Vec<MIRProjection> {
        projections.iter().map(|projection| self.build_projection(*projection)).collect()
    }

    fn build_place(&mut self, place: PlaceId) -> MIRPlace {
        let thir_place = &self.thir.places[place];
        let local = self.build_place_base(thir_place.base);
        let projections = self.build_projections(&thir_place.projections);
        let ty = self.ty(thir_place.ty);

        MIRPlace { local, projections, ty, span: thir_place.span }
    }

    fn move_or_copy(&self, place: MIRPlace) -> MIROperand {
        if place.ty.is_copy(self.db) { place.into_copy() } else { place.into_move() }
    }

    fn struct_ref(&self, struct_ref: &StructRef) -> StructRef {
        StructRef {
            def: struct_ref.def,
            args: struct_ref.args.iter().map(|ty| self.ty(*ty)).collect_vec(),
        }
    }

    fn enum_ref(&self, enum_ref: &EnumRef) -> EnumRef {
        EnumRef {
            def: enum_ref.def,
            args: enum_ref.args.iter().map(|ty| self.ty(*ty)).collect_vec(),
        }
    }

    fn get_str_struct_ref(&self) -> StructRef {
        let TypeDefId::Struct(str_def) = str_def(self.db) else { unreachable!() };
        StructRef { def: str_def, args: vec![] }
    }

    fn build_strlit(&self, strlit: &str, span: Span) -> MIRRValueKind {
        let data = MIROperand::Constant(
            MIRConstant::CString { contents: strlit.into(), null_terminated: false },
            span,
        );
        let str_ty: TypeRef = str_id(self.db).into();
        let str_ref = str_ty.as_struct_ref(self.db).expect("str is a struct");
        let len_field_ty = str_ref
            .typeof_field(self.db, Symbol::new(self.db, "len"))
            .expect("str has a len field");

        let len = MIROperand::Constant(
            MIRConstant::Integer { value: strlit.len() as u128, ty: len_field_ty },
            span,
        );

        MIRRValueKind::StructLit {
            struct_ref: self.get_str_struct_ref(),
            fields: BTreeMap::from_iter(vec![
                (Symbol::new(self.db, "data"), data),
                (Symbol::new(self.db, "len"), len),
            ]),
            span,
        }
    }

    fn build_rvalue(&mut self, expr: ExprId) -> MIRRValue {
        let thir_expr = &self.thir.exprs[expr];
        let ty = self.ty(thir_expr.ty);
        let span = thir_expr.span;
        let kind = match &thir_expr.kind {
            ExprKind::StrLit(lit) => {
                self.build_strlit(lit.interned().contents(self.db), span)
            }
            ExprKind::StructLit { struct_def, fields: thir_fields } => {
                let struct_ref = self.struct_ref(struct_def);
                let fields = self.build_fields(thir_fields);
                MIRRValueKind::StructLit { struct_ref, fields, span }
            }
            ExprKind::Constructor { enum_def, idx, args } => MIRRValueKind::Constructor {
                enum_ref: self.enum_ref(enum_def),
                idx: *idx,
                args: self.build_constructor_args(args),
                span,
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
                MIRRValueKind::Use(self.place_of_local(local, span).into_move())
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
                exprs.iter().map(|expr| self.build_operand(*expr)).collect_vec(),
                span,
            ),
            ExprKind::SizeOf(ty) => MIRRValueKind::SizeOf(self.ty(*ty)),
            ExprKind::TypeName(ty) => {
                let ty = self.ty(*ty);
                let name = ty.to_string(self.db);
                let name_len = name.len();

                let tref: TypeRef = str_id(self.db).into();
                let struct_ref = tref
                    .as_struct_ref(self.db)
                    .expect("core::io::str should be a struct");

                let data = MIROperand::Constant(
                    MIRConstant::CString { contents: name, null_terminated: false },
                    span,
                );
                let len_symb = Symbol::new(self.db, "len");
                let len_ty = struct_ref
                    .typeof_field(self.db, len_symb)
                    .expect("core::io::str struct should have field len");
                let len = MIROperand::Constant(
                    MIRConstant::Integer { value: name_len as u128, ty: len_ty },
                    span,
                );
                MIRRValueKind::StructLit {
                    struct_ref,
                    fields: BTreeMap::from_iter([
                        (Symbol::new(self.db, "data"), data),
                        (len_symb, len),
                    ]),
                    span,
                }
            }
            ExprKind::Metadata(expr) => {
                MIRRValueKind::Metadata(self.build_operand(*expr))
            }
            ExprKind::Cast(expr, ty) => {
                let as_ptr_like =
                    |ty: TypeRef| ty.as_ptr(self.db).or_else(|| ty.as_ref(self.db));
                if let Some((muta, _)) = as_ptr_like(*ty) {
                    let expr_val = &self.thir.exprs[*expr];
                    let expr_ty = expr_val.ty;
                    let (op_m, _) = as_ptr_like(expr_ty)
                        .unwrap_or((Mutability::Const, TypeRef::Error));
                    if muta.is_mut() && !op_m.is_mut() {
                        // const ptr-like to mut ptr-like
                        Diag::generic_error(format!("cannot cast a const pointer/reference type to a mutable pointer/reference type. ({} to {})", expr_ty.to_string(self.db), ty.to_string(self.db)), span)
                            .accumulate(self.db);
                    }
                }
                MIRRValueKind::Cast(self.build_operand(*expr), *ty)
            }
            _ if let Some(cst) = self.build_expr_as_constant(expr) => {
                MIRRValueKind::Use(MIROperand::Constant(cst, span))
            }
            ExprKind::SliceLit(_) => todo!(),
            ExprKind::Error => {
                println!("Thir is:");
                println!("{}", self.thir.display(self.db));
                panic!("Can only produce MIR of error-less THIR")
            }
            _ => {
                unreachable!("Unhandled expression at {}", span.start().loc_info(self.db))
            }
        };
        MIRRValue { kind, ty, span }
    }

    fn build_fields(
        &mut self,
        fields: &[ThirStructField<ExprId>],
    ) -> BTreeMap<Symbol, MIROperand> {
        fields
            .iter()
            .map(|field| {
                let operand = self.build_operand(field.expr);
                (field.field, operand)
            })
            .collect()
    }

    fn build_constructor_args(
        &mut self,
        args: &ThirConstructorArgs<ExprId>,
    ) -> MIRConstructorArgs {
        match args {
            ThirConstructorArgs::Tuple(exprs) => MIRConstructorArgs::Tuple(
                exprs.iter().map(|expr| self.build_operand(*expr)).collect_vec(),
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
        let called = called.concretize(self.db, self.subs);
        assert!(!matches!(called.id.parent(self.db), ScopeOwnerId::Interface(_)));
        let callee = MIRCallee::Direct(called);
        let args = args.iter().map(|arg| self.build_operand(*arg)).collect_vec();
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
        self.builder
            .switch_to_block(block)
            .expect("Cannot switch to already terminated block");
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

        self.goto(cond_bb);

        self.switch_to(cond_bb);
        let cond = self.build_expr_with_setup(cond);
        self.branch(cond, body_bb, merge_bb, span);

        self.switch_to(body_bb);
        self.build_stmts(body);

        if !self.current_block_is_terminated() {
            self.goto(cond_bb);
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

        if let Some(else_stmts) = else_ {
            let else_bb = self.builder.new_block(Some("if-else".into()));

            self.branch(cond, then_bb, else_bb, span);

            self.switch_to(else_bb);
            self.build_stmts(else_stmts);
            if !self.current_block_is_terminated() {
                self.goto(merge_bb);
            }
        } else {
            self.branch(cond, then_bb, merge_bb, span);
        }

        self.switch_to(then_bb);
        self.build_stmts(then);
        if !self.current_block_is_terminated() {
            self.goto(merge_bb);
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
                Either::Right(self.place_of_local(local, thir_expr.span))
            }
            _ => Either::Left(self.build_rvalue(expr)),
        }
    }

    fn assign(&mut self, dest: MIRPlace, rvalue: MIRRValue) {
        self.builder.emit(Stmt::Assign { dest, rvalue });
    }

    fn build_expr_as_constant(&self, expr: ExprId) -> Option<MIRConstant> {
        let thir_expr = &self.thir.exprs[expr];
        match &thir_expr.kind {
            ExprKind::IntLit(value) => {
                let ty = self.ty(thir_expr.ty);
                Some(MIRConstant::Integer { value: *value as u128, ty })
            }
            // TODO: Handle unicode one day
            ExprKind::Charlit(lit) => Some(MIRConstant::Integer {
                value: u128::from(*lit as u8),
                ty: char_id(self.db).into(),
            }),
            ExprKind::CStrLit(str_lit) => Some(MIRConstant::CString {
                contents: str_lit.interned().contents(self.db).clone(),
                null_terminated: true,
            }),
            ExprKind::BoolLit(value) => Some(MIRConstant::Bool(*value)),
            _ => None,
        }
    }

    fn build_operand(&mut self, expr: ExprId) -> MIROperand {
        let span = self.thir.exprs[expr].span;
        if let Some(cst) = self.build_expr_as_constant(expr) {
            return MIROperand::Constant(cst, span);
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
        self.builder
            .terminate(terminator)
            .expect("Cannot terminate already terminated block");
    }

    fn build_ret(&mut self, value: Option<MIROperand>, span: Span) {
        self.build_terminator(MIRTerminator::Return { value, span });
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

    pub fn lower(mut self) -> Mir {
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
            .finalize(MIRKey::new(self.db, self.thir.id, self.subs.to_vec()).into())
            .expect("Something went wrong finalizing the builder")
    }
}

impl TypeRef {
    /// Cheking if a type can be copied. This will be delegated to an interface
    /// check.
    pub fn is_copy(self, db: &dyn Db) -> bool {
        match self {
            Self::Concrete(type_id) => type_id.is_copy(db),
            _ => false, // TODO
        }
    }
}

impl TypeId {
    pub fn is_copy(self, db: &dyn Db) -> bool {
        if let TypeDefId::Builtin(b) = self.def(db) {
            match b.kind(db) {
                BuiltinTypeKind::Void
                | BuiltinTypeKind::Never
                | BuiltinTypeKind::Bool
                | BuiltinTypeKind::Ptr { .. }
                | BuiltinTypeKind::Int { .. } => true,
                BuiltinTypeKind::Ref { mutability } => !mutability.is_mut(),
                BuiltinTypeKind::Slice => false,
                // TODO: allow copy if all elements are copy
                #[allow(clippy::match_same_arms)]
                BuiltinTypeKind::Tuple => false,
            }
        } else {
            false
        }
    }
}

impl FunctionRef {
    #[must_use]
    pub fn concretize(mut self, db: &dyn Db, subs: &[TypeRef]) -> Self {
        let zelf = self.self_ty.map(|ty| ty.with_substitution(db, subs));
        let Some((id, new_subs)) = concretize_fid(db, self.id, &self.args, zelf) else {
            return self;
        };
        if id != self.id {
            self.id = id;
            self.args = new_subs;
        }
        self
    }
}
