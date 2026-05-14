pub mod constraints;
mod display;
pub mod expr;
mod implems;
pub mod implicit;
pub mod pattern;
mod types;
mod unify;
pub mod var;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{
    Db,
    common::symbols::Symbol,
    hir::{LocalId, PartialTypeRef, function_ast},
    name_resolve::{implems::resolve_type_expr_as_interface, type_expr::get_templates_of_fun},
    parse_tree::top_level::AstTemplateArg,
    ril::{
        FunctionId, InterfaceId, Package, ScopeOwnerId, StructId, TypeDefId, TypeId, TypeParamId,
        TypeRef,
    },
    thir::{
        ExprId, InferCallInfos,
        inference::{
            constraints::{InferenceConstraint, InferenceConstraintId},
            implicit::{AsAstImplCtx, ImplicitContext},
        },
    },
};
use ena::unify::{InPlace, UnificationTable, UnifyValue};
use itertools::Itertools;
use var::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferTy {
    Var(InferVar),
    Adt {
        def: TypeDefId,
        fields: Box<[InferTy]>,
    },
    Param(TypeParamId),
    Zelf,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct InterfaceImplem {
    pub interface: InterfaceId,
    pub ty: InferTy,
    pub templates: Box<[InferTy]>,
}

#[derive(Clone)]
pub struct InferenceCtx<'a> {
    db: &'a dyn Db,
    table: UnificationTable<InPlace<InferVar>>,
    local_map: HashMap<LocalId, InferVar>,
    implicit_ctx: Arc<ImplicitContext>,

    func: FunctionId,

    current_constraints: Vec<Arc<InferenceConstraint>>,
    all_constraints: HashMap<InferenceConstraintId, Arc<InferenceConstraint>>,
    solved_constraints: HashSet<InferenceConstraintId>,

    implements: HashMap<InterfaceId, HashSet<InterfaceImplem>>,

    templates: Arc<[AstTemplateArg]>,
    packages: Arc<[Package<'a>]>,
    call_infos: HashMap<ExprId, InferCallInfos>,
    next_constraint_id: usize,
}

impl<'db> InferenceCtx<'db> {
    fn create_local_map(
        table: &mut UnificationTable<InPlace<InferVar>>,
        locals: &[LocalId],
    ) -> HashMap<LocalId, InferVar> {
        locals.iter().map(|id| (*id, table.new_key(None))).collect()
    }

    pub fn new(
        db: &'db dyn Db,
        locals: &[LocalId],
        func: FunctionId,
        params: &'db [LocalId],
        packages: Arc<[Package<'db>]>,
    ) -> Self {
        let mut table = UnificationTable::new();
        let local_map = Self::create_local_map(&mut table, locals);

        let templates = get_templates_of_fun(db, func.interned());

        let infer_templates: Arc<[InferTy]> = templates
            .iter()
            .enumerate()
            .map(|(i, _)| InferTy::Param(TypeParamId(i)))
            .collect();

        let ctx = ImplicitContext::from_function(
            db,
            func,
            infer_templates.clone(),
            if let ScopeOwnerId::Module(_) = func.parent(db) {
                None
            } else {
                Some(InferTy::Zelf)
            },
        )
        .unwrap();

        let mut this = Self {
            db,
            table,
            local_map,
            func,
            current_constraints: Vec::new(),
            all_constraints: HashMap::new(),
            solved_constraints: HashSet::new(),
            templates: templates.clone(),
            packages,
            call_infos: HashMap::new(),
            next_constraint_id: 0,
            implements: HashMap::new(),
            implicit_ctx: Arc::new(ctx),
        };

        let ast = function_ast(this.db, func.interned()).inner(this.db);
        let args = ast.get_args();
        for (local, ast) in params.iter().zip_eq(args) {
            let ty = this.implicit_ctx().resolve(this.db, &ast.ty.data).unwrap();
            let ty = this.allocate_type_ref(&ty, &this.implicit_ctx());
            let local_ty = this.infer_local(*local);
            this.unify(local_ty, ty).unwrap();
        }

        infer_templates
            .iter()
            .zip(templates.as_ref())
            .for_each(|(infer_ty, ast)| {
                let constraints = &ast.constraints;
                for cons in constraints {
                    let resolved = resolve_type_expr_as_interface(
                        this.db,
                        &cons,
                        this.implicit_ctx().owner_module(this.db).interned(),
                        templates.as_ref(),
                        false,
                    )
                    .unwrap();
                    let interface_id = resolved.def(this.db);
                    let interface_args = resolved
                        .args(this.db)
                        .iter()
                        .map(|t_ref| this.allocate_type_ref(t_ref, this.implicit_ctx().as_ref()))
                        .collect::<Box<[_]>>();
                    this.add_implementation(interface_id, infer_ty.clone(), &interface_args);
                }
            });

        this
    }

    pub fn templates(&self) -> Arc<[InferTy]> {
        self.implicit_ctx.get_templates()
    }

    pub fn zelf(&self) -> Option<InferTy> {
        self.implicit_ctx().zelf().cloned()
    }

    pub fn implicit_ctx(&self) -> Arc<ImplicitContext> {
        self.implicit_ctx.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum UnificationError {
    TypeDefIdMismatch(TypeDefId, TypeDefId),
    FieldCountMismatch(usize, usize),
    RecursiveDefinition(InferVar),
    UnmetConstraint(Arc<InferenceConstraint>, Box<UnificationError>),
    ExpectedPtrLike(TypeDefId),
    MinTupleLengthMismatch { expected: usize, got: usize },
    ExpectedStructWithField { def: TypeDefId, field: Symbol },
    IncompleteStructLit { id: StructId, missing: Symbol },
    NonStructForStructLit(PartialTypeRef),
    TemplateDereferencing(TypeParamId),
    TemplateConstraining(TypeParamId),
    ArgCountMismatch(FunctionId, usize),
    StaticMethodCallOnReceiver(ExprId, FunctionId),
    NoImplemCandidateFor(InferTy, InterfaceId, Box<[InferTy]>),
    ZelfConstraining,
}

impl<'db> InferenceCtx<'db> {
    pub fn snapshot<T>(
        &mut self,
        f: impl Fn(&mut Self) -> Result<T, UnificationError>,
    ) -> Result<T, UnificationError> {
        let snapshot = self.table.snapshot();
        match f(self) {
            Ok(res) => {
                self.table.commit(snapshot);
                Ok(res)
            }
            Err(err) => {
                self.table.rollback_to(snapshot);
                Err(err)
            }
        }
    }

    pub fn solve(&mut self, ty: InferTy) -> Option<TypeRef> {
        let ty = self.find(&ty);
        match ty {
            InferTy::Var(_) => None,
            InferTy::Adt { def, fields } => Some(TypeRef::Concrete(TypeId::new(
                self.db,
                def,
                fields
                    .into_iter()
                    .map(|field| self.solve(field))
                    .collect::<Option<Vec<_>>>()?,
            ))),
            InferTy::Param(type_param_id) => Some(TypeRef::Param(type_param_id)),
            InferTy::Zelf => self.zelf().map(|_| TypeRef::Zelf),
        }
    }
}

impl InferTy {
    pub fn is_adt(&self) -> Option<(TypeDefId, &[InferTy])> {
        match self {
            InferTy::Adt { def, fields } => Some((*def, fields)),
            _ => None,
        }
    }
}
