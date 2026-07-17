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
    sync::Arc,
};

use crate::{
    Db,
    common::symbols::Symbol,
    hir::{LocalId, Mutability, function_ast},
    name_resolve::{
        definition::Definition,
        implems::resolve_type_expr_as_interface,
        type_expr::{
            get_templates_of_fun, get_templates_of_fun_only, templates_of_enum,
            templates_of_struct,
        },
    },
    parse_tree::top_level::AstTemplateArg,
    printer::type_printer::TypePrinter,
    ril::{
        BuiltinTypeId, BuiltinTypeKind, FunctionId, InterfaceId, Package, StructId,
        TypeDefId, TypeId, TypeParamId, TypeRef, display::Display,
    },
    typecheck::{
        ExprId, InferCallInfos, PatternId, PlaceId,
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
    Adt { def: TypeDefId, fields: Vec<InferTy> },
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
pub struct InferenceCtx<'a> {
    db: &'a dyn Db,
    table: UnificationTable<InPlace<InferVar>>,
    local_map: BTreeMap<LocalId, InferVar>,
    implicit_ctx: Arc<ImplicitContext>,

    all_constraints: BTreeMap<InferenceConstraintId, Arc<InferenceConstraint>>,
    solved_constraints: BTreeSet<InferenceConstraintId>,
    error_constraints: BTreeSet<InferenceConstraintId>,
    listeners: BTreeMap<InferVar, Vec<InferenceConstraintId>>,
    ready: VecDeque<InferenceConstraintId>,
    ready_set: HashSet<InferenceConstraintId>,

    implements: BTreeMap<InterfaceId, HashSet<InterfaceImplem>>,
    call_infos: BTreeMap<ExprId, InferCallInfos>,
    next_constraint_id: usize,
    impl_depth: usize,

    pub inferred_exprs: BTreeMap<ExprId, InferTy>,
    pub inferred_patterns: BTreeMap<PatternId, InferTy>,
    pub inferred_places: BTreeMap<PlaceId, InferTy>,

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
    ) -> Self {
        let mut table = UnificationTable::new();
        let local_map = Self::create_local_map(&mut table, locals);

        let templates = get_templates_of_fun(db, func.interned());

        let infer_templates: Arc<[InferTy]> = templates
            .iter()
            .enumerate()
            .map(|(i, _)| InferTy::Param(TypeParamId(i)))
            .collect();

        let l =
            infer_templates.len() - get_templates_of_fun_only(db, func.interned()).len();

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
            error_constraints: BTreeSet::new(),
            call_infos: BTreeMap::new(),
            next_constraint_id: 0,
            implements: BTreeMap::new(),
            implicit_ctx: Arc::new(ctx),
            impl_depth: 0,
            listeners: BTreeMap::new(),
            ready: VecDeque::new(),
            ready_set: HashSet::new(),
            in_flight_impls: HashSet::new(),
            inferred_exprs: BTreeMap::new(),
            inferred_patterns: BTreeMap::new(),
            inferred_places: BTreeMap::new(),
        };

        let ast = function_ast(this.db, func.interned()).inner(this.db);
        let args = ast.get_args();
        for (local, ast) in params.iter().zip_eq(args) {
            let ty = this.implicit_ctx().resolve(this.db, &ast.ty.data).unwrap_or_else(||
                panic!(
                    "Top level items should already have valid and resolved types ({})",
                    ast.ty.span.start().loc_info(db)
                )
            );
            let ty = this.allocate_type_ref(ty, &this.implicit_ctx());
            let local_ty = this.infer_local(*local);
            this.unify(local_ty, ty)
                .expect("First local type unification should not fail");
        }

        #[cfg(debug_assertions)]
        for (local, var) in &this.local_map {
            if params.contains(local)
            /* or however params are identifiable */
            {
                debug_assert!(
                    this.table.probe_value(*var).is_some(),
                    "param local {local:?} left unseeded in `{}`",
                    func.name(db).display(db),
                );
            }
        }

        match (func.receiver(db).as_zelf_arg(), zelf_ty) {
            (Some(arg), Some(zelf_ty)) => {
                let ty = arg.get_zelf_type_for(db, zelf_ty);
                let local_ty = this.local_var(
                    zelf.expect("function has a receiver but no self local was provided"),
                );
                this.unify(ty, InferTy::Var(local_ty))
                    .expect("seeding self's declared type should not fail");
            }
            (Some(_), None) => panic!(
                "method `{}` has a receiver but its impl has no canonical Self \
                 (get_canonical_zelf returned None — likely a generic impl header \
                 that failed to resolve)",
                func.name(db).display(db),
            ),
            (None, _) if zelf.is_some() => panic!(
                "HIR provided a self local for `{}` but the AST receiver is None \
                 (as_zelf_arg fell through — check receiver variant coverage)",
                func.name(db).display(db),
            ),
            (None, _) => {} // free function, nothing to seed
        }

        infer_templates
            .iter()
            .zip(templates)
            .for_each(|(infer_ty, ast)| {
                let constraints = &ast.constraints;
                for cons in constraints {
                    let resolved = resolve_type_expr_as_interface(
                        this.db,
                        cons,
                        this.implicit_ctx().owner_module(this.db),
                        templates.as_ref(),
                        false,
                    )
                    .expect("Top level template argument constraints should already be resolved to interfaces");
                    let interface_id = resolved.def(this.db);
                    let interface_args = resolved
                        .args(this.db)
                        .iter()
                        .map(|t_ref| this.allocate_type_ref(*t_ref, this.implicit_ctx().as_ref()))
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

    pub fn unsolved_constraints(&self) -> Vec<Arc<InferenceConstraint>> {
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
    NonStructForStructLit(TypeRef),
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
            UnificationError::RecursiveDefinition(infer_var) => {
                write!(f, "Recursive definition: {}", infer_var)
            }
            UnificationError::UnmetConstraint(
                inference_constraint,
                unification_error,
            ) => {
                write!(
                    f,
                    "Unmet constraint: {} (reason: {})",
                    inference_constraint.id.0,
                    unification_error.display(self.db)
                )
            }
            UnificationError::ExpectedPtrLike(type_def_id) => write!(
                f,
                "Expected ptr-like: {}",
                type_def_id.name(self.db).display(self.db)
            ),
            UnificationError::MinTupleLengthMismatch { expected, got } => {
                write!(f, "Min tuple length mismatch: expexted {expected} but got {got}")
            }
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
        let old_ready_set = self.ready_set.clone();
        let old_exprs = self.inferred_exprs.clone();
        let old_patterns = self.inferred_patterns.clone();
        let old_places = self.inferred_places.clone();
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
                self.ready_set = old_ready_set;
                self.inferred_exprs = old_exprs;
                self.inferred_patterns = old_patterns;
                self.inferred_places = old_places;
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

    pub fn get_templates_for(&mut self, def: Definition) -> Vec<InferTy> {
        let asts: &[AstTemplateArg] = match def {
            Definition::Function(func) => get_templates_of_fun(self.db, func.interned()),
            Definition::Interface(_) => todo!(),
            Definition::Module(_) => {
                return Vec::new();
            }
            Definition::Type(tdef) => match tdef {
                TypeDefId::Builtin(_) => {
                    return Vec::new();
                }
                TypeDefId::Struct(struct_id) => {
                    templates_of_struct(self.db, struct_id.interned())
                }
                TypeDefId::Enum(enum_id) => {
                    templates_of_enum(self.db, enum_id.interned())
                }
            },
        };

        // Just do that for the moment, in the future it would be nice to apply
        // interface constraints etc on the type :)

        asts.iter().map(|_| InferTy::Var(self.fresh_var())).collect()
    }
}

impl InferTy {
    pub fn is_adt(&self) -> bool {
        matches!(self, InferTy::Adt { .. })
    }

    pub fn as_adt(&self) -> Option<(TypeDefId, &[InferTy])> {
        match self {
            InferTy::Adt { def, fields } => Some((*def, fields)),
            _ => None,
        }
    }

    pub fn as_builtin(&self) -> Option<(BuiltinTypeId, &[InferTy])> {
        match self {
            InferTy::Adt { def: TypeDefId::Builtin(def), fields } => Some((*def, fields)),
            _ => None,
        }
    }

    pub fn as_ref<'a>(&'a self, db: &dyn Db) -> Option<(Mutability, &'a InferTy)> {
        let (def, args) = self.as_adt()?;
        let mutability = def.is_ptr_like(db)?.mutability();
        assert_eq!(args.len(), 1);
        Some((mutability, &args[0]))
    }

    pub fn as_ref_slice<'a>(&'a self, db: &dyn Db) -> Option<(Mutability, &'a InferTy)> {
        let (muta, ty) = self.as_ref(db)?;
        let as_slice = ty.as_slice(db)?;
        Some((muta, as_slice))
    }

    pub fn as_slice<'a>(&'a self, db: &dyn Db) -> Option<&'a InferTy> {
        let (def, args) = self.as_builtin()?;
        match def.kind(db) {
            BuiltinTypeKind::Slice => Some(&args[0]),
            _ => None,
        }
    }

    pub fn as_concrete(&self, db: &dyn Db) -> Option<TypeId> {
        match self {
            InferTy::Var(_) => None,
            InferTy::Adt { def, fields } => {
                let fields: Option<Vec<_>> = fields
                    .iter()
                    .map(|ty| ty.as_concrete(db).map(|ty| ty.into()))
                    .collect();
                let fields = fields?;
                Some(TypeId::new(db, *def, fields))
            }
            InferTy::Param(_) => None,
        }
    }
}
