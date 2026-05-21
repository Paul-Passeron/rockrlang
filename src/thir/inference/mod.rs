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

pub mod canon;
pub mod constraints;
pub mod expr;
mod implems;
pub mod implicit;
pub mod pattern;
mod types;
mod unify;
pub mod var;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fmt, mem,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    Db, SourceFile,
    common::{location::Span, symbols::Symbol},
    compiler::diagnostic::{Label, Severity},
    hir::{LocalId, PartialTypeRef, function_ast},
    name_resolve::{
        implems::resolve_type_expr_as_interface,
        type_expr::{get_templates_of_fun, get_templates_of_fun_only},
    },
    printer::type_printer::TypePrinter,
    ril::{
        FileModule, FunctionId, InterfaceId, Package, StructId, TypeDefId, TypeId, TypeParamId,
        TypeRef, display::Display,
    },
    thir::{
        Diagnostic, ExprId, InferCallInfos,
        inference::{
            canon::CanonTy,
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
}

impl InferTy {
    pub fn to_string<'a>(&'a self, db: &'a dyn crate::Db) -> String {
        TypePrinter::new().infer_ty_to_string(db, self.clone(), None)
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct InterfaceImplem {
    pub interface: InterfaceId,
    pub ty: InferTy,
    pub templates: Box<[InferTy]>,
}

#[derive(Clone)]
pub struct DiagnosticEngine<'db> {
    pub diags: Vec<Diagnostic>,
    pub db: &'db dyn Db,
    pub packages: Arc<[Package<'db>]>,
}

impl<'db> DiagnosticEngine<'db> {
    pub fn new(db: &'db dyn Db, packages: Arc<[Package<'db>]>) -> Self {
        Self {
            diags: Vec::new(),
            db,
            packages,
        }
    }

    pub fn drain(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diags)
    }

    pub fn push_regular_diagnostic_with_message_and_primary(
        &mut self,
        err: String,
        primary: Option<String>,
        span: Span,
    ) {
        self.diags.push(Diagnostic {
            severity: Severity::Error,
            message: err,
            primary: Label {
                span,
                message: primary,
            },
            secondary: vec![],
            notes: vec![],
            help: vec![],
        })
    }

    pub fn push_regular_diagnostic_with_message(&mut self, err: String, span: Span) {
        self.push_regular_diagnostic_with_message_and_primary(err, None, span);
    }

    pub fn push_regular_diagnostic(&mut self, err: UnificationError, span: Span) {
        self.push_regular_diagnostic_with_message(err.display(self.db).to_string(), span);
    }

    fn find_file(&self, path: impl AsRef<Path>) -> Option<SourceFile> {
        let canon = path.as_ref().canonicalize().ok()?;
        fn handle_submodule(
            this: &DiagnosticEngine,
            sub: &FileModule,
            path: &PathBuf,
        ) -> Option<SourceFile> {
            if sub.file(this.db).path(this.db) == path {
                return Some(sub.file(this.db));
            }
            for submodule in sub.submodules(this.db) {
                if let Some(res) = handle_submodule(this, submodule, path) {
                    return Some(res);
                }
            }
            None
        }
        for package in self.packages.iter() {
            if let Some(res) = handle_submodule(self, &package.root(self.db), &canon) {
                return Some(res);
            }
        }
        None
    }
}

#[derive(Clone)]
pub struct InferenceCtx<'a> {
    db: &'a dyn Db,
    table: UnificationTable<InPlace<InferVar>>,
    local_map: BTreeMap<LocalId, InferVar>,
    implicit_ctx: Arc<ImplicitContext>,

    // current_constraints: Vec<Arc<InferenceConstraint>>,
    all_constraints: BTreeMap<InferenceConstraintId, Arc<InferenceConstraint>>,
    solved_constraints: BTreeSet<InferenceConstraintId>,
    listeners: BTreeMap<InferVar, Vec<InferenceConstraintId>>,
    ready: VecDeque<InferenceConstraintId>,
    ready_set: HashSet<InferenceConstraintId>,

    implements: BTreeMap<InterfaceId, HashSet<InterfaceImplem>>,
    packages: Arc<[Package<'a>]>,
    call_infos: BTreeMap<ExprId, InferCallInfos>,
    next_constraint_id: usize,
    impl_depth: usize,

    pub inferred_exprs: BTreeMap<ExprId, InferTy>,

    pub(super) diagnostics: DiagnosticEngine<'a>,
    in_flight_impls: HashSet<(InterfaceId, CanonTy)>,
}

impl<'db> InferenceCtx<'db> {
    fn create_local_map(
        table: &mut UnificationTable<InPlace<InferVar>>,
        locals: &[LocalId],
    ) -> BTreeMap<LocalId, InferVar> {
        locals.iter().map(|id| (*id, table.new_key(None))).collect()
    }

    pub fn new(
        db: &'db dyn Db,
        locals: &[LocalId],
        func: FunctionId,
        zelf: Option<LocalId>,
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

        let l = infer_templates.len() - get_templates_of_fun_only(db, func.interned()).len();

        let owner_ctx = ImplicitContext::new(
            db,
            func.parent(db),
            Arc::new([]),
            infer_templates.iter().take(l).cloned().collect(),
            None,
        )
        .unwrap();

        let zelf_ty = func
            .parent(db)
            .get_canonical_zelf(db)
            .map(|ty| Self::static_allocate_type_ref(db, &ty, &owner_ctx).unwrap());

        let ctx =
            ImplicitContext::from_function(db, func, infer_templates.clone(), zelf_ty.clone())
                .expect(
                    "Could not create implicit context for inference context. This should not fail",
                );

        let mut this = Self {
            db,
            table,
            local_map,
            all_constraints: BTreeMap::new(),
            solved_constraints: BTreeSet::new(),
            packages: packages.clone(),
            call_infos: BTreeMap::new(),
            next_constraint_id: 0,
            implements: BTreeMap::new(),
            implicit_ctx: Arc::new(ctx),
            impl_depth: 0,
            diagnostics: DiagnosticEngine::new(db, packages),
            listeners: BTreeMap::new(),
            ready: VecDeque::new(),
            ready_set: HashSet::new(),
            in_flight_impls: HashSet::new(),
            inferred_exprs: BTreeMap::new(),
        };

        let ast = function_ast(this.db, func.interned()).inner(this.db);
        let args = ast.get_args();
        for (local, ast) in params.iter().zip_eq(args) {
            let ty = this.implicit_ctx().resolve(this.db, &ast.ty.data).expect(
                format!(
                    "Top level items should already have valid and resolved types ({})",
                    ast.ty.span.start().loc_info(db)
                )
                .as_str(),
            );
            let ty = this.allocate_type_ref(&ty, &this.implicit_ctx());
            let local_ty = this.infer_local(*local);
            this.unify(local_ty, ty)
                .expect("First local type unification should not fail");
        }

        let actual_zelf_ty = func
            .receiver(db)
            .as_zelf_arg()
            .map(|arg| arg.get_zelf_type_for(db, zelf_ty.unwrap()));

        actual_zelf_ty.map(|ty| {
            let local_ty = this.local_var(zelf.unwrap());
            this.unify(ty, InferTy::Var(local_ty)).unwrap();
        });

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
                    .expect("Top level template argument constraints should already be resolved to interfaces");
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

    pub(super) fn drain_call_infos(&mut self) -> BTreeMap<ExprId, InferCallInfos> {
        mem::take(&mut self.call_infos)
    }

    pub fn implicit_ctx(&self) -> Arc<ImplicitContext> {
        self.implicit_ctx.clone()
    }

    pub fn unsolved_constraints(&self) -> Box<[Arc<InferenceConstraint>]> {
        self.all_constraints
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
            .difference(&self.solved_constraints)
            .map(|id| self.all_constraints[id].clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Hash)]
#[allow(dead_code)]
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
    InvalidStructField { id: StructId, invalid: Symbol },
    AlreadyDiagnosed,
    Custom(String),
}

impl UnificationError {
    pub fn display<'a, 'db>(&'a self, db: &'db dyn Db) -> Display<'db, &'a Self> {
        Display { value: self, db }
    }
}

impl fmt::Display for Display<'_, &UnificationError> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            UnificationError::TypeDefIdMismatch(type_def_id, type_def_id1) => {
                write!(
                    f,
                    "TypeDefId mismatch: {} != {}",
                    type_def_id.name(self.db).display(self.db),
                    type_def_id1.name(self.db).display(self.db)
                )
            }
            UnificationError::FieldCountMismatch(x, y) => {
                write!(f, "Field count mismatch: {} != {}", x, y)
            }
            UnificationError::RecursiveDefinition(infer_var) => write!(
                f,
                "Recursive definition: {}",
                InferTy::Var(*infer_var).to_string(self.db)
            ),
            UnificationError::UnmetConstraint(inference_constraint, unification_error) => {
                write!(
                    f,
                    "Unmet constraint: {} (reason: {})",
                    inference_constraint.kind.display(self.db),
                    unification_error.display(self.db)
                )
            }
            UnificationError::ExpectedPtrLike(type_def_id) => write!(
                f,
                "Expected ptr-like: {}",
                type_def_id.name(self.db).display(self.db)
            ),
            UnificationError::MinTupleLengthMismatch { expected, got } => write!(
                f,
                "Min tuple length mismatch: expexted {expected} but got {got}"
            ),
            UnificationError::ExpectedStructWithField { def, field } => write!(
                f,
                "Expected struct with field: {} (field: {})",
                def.name(self.db).display(self.db),
                field.display(self.db)
            ),
            UnificationError::IncompleteStructLit { id, missing } => write!(
                f,
                "Incomplete struct lit: {} (missing: {})",
                id.name(self.db).display(self.db),
                missing.display(self.db)
            ),
            UnificationError::InvalidStructField { id, invalid } => {
                write!(
                    f,
                    "Invalid field in struct lit: {} (invalid: {})",
                    id.name(self.db).display(self.db),
                    invalid.display(self.db)
                )
            }

            UnificationError::NonStructForStructLit(partial_type_ref) => {
                write!(f, "Non struct for struct-lit: {:?}", partial_type_ref)
            }
            UnificationError::TemplateDereferencing(type_param_id) => {
                write!(f, "Template T{} dereferenced", type_param_id.0)
            }
            UnificationError::TemplateConstraining(type_param_id) => {
                write!(f, "Template T{} constrained", type_param_id.0)
            }
            UnificationError::ArgCountMismatch(function_id, count) => {
                write!(
                    f,
                    "Argument count mismatch for function {} (expected {count})",
                    function_id.name(self.db).display(self.db)
                )
            }
            UnificationError::StaticMethodCallOnReceiver(expr_id, function_id) => {
                write!(
                    f,
                    "Static method call on receiver: {:?} (function: {})",
                    expr_id,
                    function_id.name(self.db).display(self.db)
                )
            }
            UnificationError::NoImplemCandidateFor(infer_ty, interface_id, items) => {
                write!(
                    f,
                    "No implementation candidate found for type {} with interface {}{}",
                    infer_ty.to_string(self.db),
                    interface_id.name(self.db).display(self.db),
                    if items.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "<{}>",
                            items.iter().map(|i| i.to_string(self.db)).join(", ")
                        )
                    }
                )
            }
            UnificationError::AlreadyDiagnosed => Ok(()),
            UnificationError::Custom(s) => write!(f, "Custom : {s}"),
        }
    }
}

impl<'db> InferenceCtx<'db> {
    pub fn snapshot<T>(
        &mut self,
        f: impl Fn(&mut Self) -> Result<T, UnificationError>,
    ) -> Result<T, UnificationError> {
        let old_listeners = self.listeners.clone();
        let old_ready = self.ready.clone();
        let old_exprs = self.inferred_exprs.clone();
        let snapshot = self.table.snapshot();
        match f(self) {
            Ok(res) => {
                self.table.commit(snapshot);
                Ok(res)
            }
            Err(err) => {
                self.table.rollback_to(snapshot);
                self.listeners = old_listeners;
                self.ready = old_ready;
                self.inferred_exprs = old_exprs;
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
