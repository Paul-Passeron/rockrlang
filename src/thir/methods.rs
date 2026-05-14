use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use crate::{
    Db,
    common::symbols::Symbol,
    hir::{PartialTypeArg, PartialTypeRef, impl_items},
    name_resolve::implems::impls_in_package,
    parse_tree::top_level::AstImplItem,
    ril::{
        FunctionId, ImplId, InterfaceRef, Package, ScopeOwnerId, TypeDefId, TypeId, TypeParamId,
        TypeRef,
        display::{Display, RilDisplay},
    },
    thir::{InferTy, TyVarId},
};

impl PartialTypeRef {
    pub fn display<'a>(&'a self, db: &'a dyn Db) -> Display<'a, &'a Self> {
        Display { value: self, db }
    }
}

impl PartialTypeArg {
    pub fn display<'a>(&'a self, db: &'a dyn Db) -> Display<'a, &'a Self> {
        Display { value: self, db }
    }
}

impl fmt::Display for Display<'_, &PartialTypeRef> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            PartialTypeRef::Resolved(type_ref) => write!(f, "{}", type_ref.display(self.db)),
            PartialTypeRef::WithHoles { def, args } => {
                write!(f, "{}", def.name(self.db).display(self.db))?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg.display(self.db))?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for Display<'_, &PartialTypeArg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            PartialTypeArg::Known(type_ref) => write!(f, "{}", type_ref.display(self.db)),
            PartialTypeArg::Partial(partial_type_ref) => {
                write!(f, "{}", partial_type_ref.display(self.db))
            }
            PartialTypeArg::Infer => write!(f, "_"),
        }
    }
}

#[salsa::tracked]
pub fn find_method_for_partial_ref<'db>(
    db: &'db dyn Db,
    ty: InferTy,
    method: Symbol,
    packages: Vec<Package<'db>>,
) -> Vec<(FunctionId, ImplMatchConstraints)> {
    let mut res = vec![];
    for implem in packages
        .iter()
        .map(|package| impls_in_package(db, *package))
        .flatten()
    {
        let matcher = implem.id(db).implemented(db);
        let templates = implem
            .id(db)
            .templates(db)
            .into_iter()
            .map(|x| x.into_iter().collect())
            .collect::<Box<[_]>>();
        if let Some(constraints) = partial_ty_pattern_matches(db, &ty, matcher, &templates) {
            let items = impl_items(db, implem.id(db).interned());
            for item in items {
                match item {
                    AstImplItem::Fundef(fundef) => {
                        if fundef.data.name == method {
                            let id = FunctionId::new(db, method, ScopeOwnerId::Impl(implem.id(db)));
                            res.push((id, constraints.clone()));
                        }
                    }
                    _ => (),
                }
            }
        }
    }
    res
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum ImplMatchConstraint {
    Unify(InferTy, InferTy),
    Implements(InferTy, InterfaceRef),
}

#[derive(Clone, PartialEq, Eq)]
pub struct ImplMatchConstraints {
    pub substitution: Vec<Option<InferTy>>,
    pub constraints: HashSet<ImplMatchConstraint>,
}

impl ImplMatchConstraints {
    pub fn new(
        substitution: Vec<Option<InferTy>>,
        constraints: HashSet<ImplMatchConstraint>,
    ) -> Self {
        Self {
            substitution,
            constraints,
        }
    }
}

pub fn allocate_type_id(db: &dyn Db, ty: TypeId) -> InferTy {
    let args = ty
        .args(db)
        .into_iter()
        .map(|ty| match ty {
            TypeRef::Error => InferTy::Error,
            _ => InferTy::Var(TyVarId::alloc()),
        })
        .collect::<Vec<_>>();
    InferTy::Adt {
        def: ty.def(db),
        args,
    }
}

pub(self) fn partial_ty_pattern_matches(
    db: &dyn Db,
    ty: &InferTy,
    matcher: TypeRef,
    templates: &[HashSet<InterfaceRef>],
) -> Option<ImplMatchConstraints> {
    fn _aux(
        db: &dyn Db,
        ty: &InferTy,
        matcher: TypeRef,
        constraints: &mut HashSet<ImplMatchConstraint>,
        subst: &mut HashMap<usize, InferTy>,
    ) -> bool {
        match (ty, matcher) {
            (_, TypeRef::Error) => false,
            (InferTy::Error, _) => false,
            (infer_ty, TypeRef::Param(type_param_id)) => {
                if let Some(other_ty) = subst.get(&type_param_id.0)
                    && other_ty != infer_ty
                {
                    constraints.insert(ImplMatchConstraint::Unify(
                        other_ty.clone(),
                        infer_ty.clone(),
                    ));
                } else {
                    subst.insert(type_param_id.0, infer_ty.clone());
                }
                // TODO: We should check that infer_ty and other_ty do not collide
                // This is fine for now as it will be caught when trying to unify the
                // constraints we got
                true
            }
            (InferTy::Param(type_param_id), TypeRef::Concrete(type_id)) => {
                constraints.insert(ImplMatchConstraint::Unify(
                    InferTy::Param(*type_param_id),
                    allocate_type_id(db, type_id),
                ));
                true
            }
            (InferTy::Var(ty_var_id), TypeRef::Concrete(type_id)) => {
                // Should hold true
                constraints.insert(ImplMatchConstraint::Unify(
                    InferTy::Var(*ty_var_id),
                    allocate_type_id(db, type_id),
                ));
                true
            }
            (InferTy::Adt { def, args }, TypeRef::Concrete(type_id)) => {
                if *def != type_id.def(db) {
                    return false;
                }
                for (arg, other_arg) in args.iter().zip(type_id.args(db).iter()) {
                    if !_aux(db, arg, *other_arg, constraints, subst) {
                        return false;
                    }
                }
                true
            }
            (InferTy::RefOrDerefLike { .. }, _) => todo!(),
            (InferTy::Deref(_), _) => todo!(),
        }
    }
    let mut constrs = HashSet::new();
    let mut substs = HashMap::new();
    if _aux(db, ty, matcher, &mut constrs, &mut substs) {
        let substitution = templates
            .iter()
            .enumerate()
            .map(|(i, interfaces)| {
                substs.get(&i).map(|infer_ty| {
                    interfaces.iter().for_each(|interface| {
                        // insert the necessary constraints on the
                        // template types that should implement the interfaces
                        constrs.insert(ImplMatchConstraint::Implements(
                            infer_ty.clone(),
                            *interface,
                        ));
                    });
                    infer_ty.clone()
                })
            })
            .collect();
        Some(ImplMatchConstraints {
            substitution,
            constraints: constrs,
        })
    } else {
        None
    }
}
