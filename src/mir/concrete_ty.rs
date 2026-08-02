use std::{collections::HashMap, marker::PhantomData};

use crate::{
    Db,
    check::thir::sanity_check::ConstructorType,
    common::symbols::Symbol,
    name_resolve::type_expr::{struct_item, templates_of_struct},
    resolved::{
        EnumId, InternedStructId, ScopeOwnerId, StructId, TypeDefId, TypeId, TypeRef,
    },
    thir,
    typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConcreteTy(salsa::Id);

#[salsa::interned]
pub struct InternedConcreteTy<'db> {
    #[returns(copy)]
    pub def: TypeDefId,
    #[returns(deref)]
    pub args: Vec<ConcreteTy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Substitution(Box<[ConcreteTy]>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConcreteStructRef(salsa::Id);

#[salsa::interned]
pub struct InternedConcreteStructRef<'db> {
    #[returns(copy)]
    pub def: StructId,
    #[returns(deref)]
    pub args: Vec<ConcreteTy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConcreteEnumRef(salsa::Id);

#[salsa::interned]
pub struct InternedConcreteEnumRef<'db> {
    #[returns(copy)]
    pub def: EnumId,
    #[returns(deref)]
    pub args: Vec<ConcreteTy>,
}

impl Substitution {
    pub fn new<T: IntoIterator<Item = ConcreteTy>>(t: T) -> Self {
        Self(t.into_iter().collect())
    }

    pub fn empty() -> Self {
        Self(Box::new([]))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = ConcreteTy> {
        self.0.iter().copied()
    }
}

impl Default for Substitution {
    fn default() -> Self {
        Self::empty()
    }
}

impl ConcreteTy {
    pub fn interned<'a>(self) -> InternedConcreteTy<'a> {
        InternedConcreteTy(self.0, PhantomData)
    }
}

impl<'a> From<InternedConcreteTy<'a>> for ConcreteTy {
    fn from(value: InternedConcreteTy<'a>) -> Self {
        Self(value.0)
    }
}

impl<'a> From<ConcreteTy> for InternedConcreteTy<'a> {
    fn from(value: ConcreteTy) -> Self {
        value.interned()
    }
}

impl ConcreteTy {
    pub fn def(self, db: &dyn Db) -> TypeDefId {
        self.interned().def(db)
    }

    pub fn args(self, db: &dyn Db) -> &[ConcreteTy] {
        self.interned().args(db)
    }

    pub fn as_type_ref(self, db: &dyn Db) -> TypeRef {
        self.interned().as_type_ref(db)
    }

    pub fn as_type_id(self, db: &dyn Db) -> TypeId {
        self.interned().as_type_id(db)
    }

    pub fn new(db: &dyn Db, def: TypeDefId, args: Vec<ConcreteTy>) -> Self {
        InternedConcreteTy::new(db, def, args).into()
    }
}

impl TypeRef {
    pub fn to_concrete(self, db: &dyn Db, subs: &Substitution) -> Option<ConcreteTy> {
        match self {
            TypeRef::Concrete(type_id) => {
                let args = type_id
                    .args(db)
                    .iter()
                    .map(|ty| ty.to_concrete(db, subs))
                    .collect::<Option<Vec<_>>>()?;
                Some(ConcreteTy::new(db, type_id.def(db), args))
            }

            TypeRef::Param(type_param_id) => subs.0.get(type_param_id.0).copied(),
            _ => None,
        }
    }
}

impl ConcreteTy {
    pub fn as_enum(self, db: &dyn Db) -> Option<ConcreteEnumRef> {
        self.interned().as_enum(db)
    }

    pub fn as_struct(self, db: &dyn Db) -> Option<ConcreteStructRef> {
        self.interned().as_struct(db)
    }

    pub fn as_tuple_ref(self, db: &dyn Db) -> Option<Vec<ConcreteTy>> {
        self.as_type_ref(db).as_tuple_ref(db).map(|tys| {
            tys.iter()
                .map(|ty| {
                    ty.to_concrete(db, &Substitution::empty())
                        .expect("Tuple element of a concrete type must be concrete")
                })
                .collect()
        })
    }

    pub fn as_ptr(self, db: &dyn Db) -> Option<(crate::hir::Mutability, ConcreteTy)> {
        self.as_type_ref(db).as_ptr(db).map(|(m, ty)| {
            (
                m,
                ty.to_concrete(db, &Substitution::empty())
                    .expect("Pointee of a concrete type must be concrete"),
            )
        })
    }

    pub fn as_ref(self, db: &dyn Db) -> Option<(crate::hir::Mutability, ConcreteTy)> {
        self.as_type_ref(db).as_ref(db).map(|(m, ty)| {
            (
                m,
                ty.to_concrete(db, &Substitution::empty())
                    .expect("Referent of a concrete type must be concrete"),
            )
        })
    }

    pub fn is_copy(self, db: &dyn Db) -> bool {
        self.interned().is_copy(db)
    }

    pub fn is_drop(self, db: &dyn Db) -> bool {
        self.interned().is_drop(db)
    }
}

#[salsa::tracked]
impl<'db> InternedConcreteTy<'db> {
    #[salsa::tracked(returns(copy))]
    pub fn as_enum(self, db: &'db dyn Db) -> Option<ConcreteEnumRef> {
        match self.def(db) {
            TypeDefId::Enum(enum_id) => {
                Some(ConcreteEnumRef::new(db, enum_id, self.args(db).to_vec()))
            }
            _ => None,
        }
    }

    #[salsa::tracked(returns(copy))]
    pub fn as_struct(self, db: &dyn Db) -> Option<ConcreteStructRef> {
        match self.def(db) {
            TypeDefId::Struct(struct_id) => {
                Some(ConcreteStructRef::new(db, struct_id, self.args(db).to_vec()))
            }
            _ => None,
        }
    }

    #[salsa::tracked(returns(copy))]
    pub fn as_type_ref(self, db: &dyn Db) -> TypeRef {
        self.as_type_id(db).into()
    }

    #[salsa::tracked(returns(copy))]
    pub fn as_type_id(self, db: &dyn Db) -> TypeId {
        TypeId::new(
            db,
            self.def(db),
            self.args(db).iter().map(|arg| arg.as_type_ref(db)).collect(),
        )
    }

    #[salsa::tracked(returns(copy))]
    pub fn is_copy(self, db: &dyn Db) -> bool {
        self.as_type_ref(db).is_copy(db)
    }

    #[salsa::tracked(returns(copy))]
    pub fn is_drop(self, db: &dyn Db) -> bool {
        self.as_type_ref(db).is_drop(db)
    }
}

impl<'db> From<InternedConcreteEnumRef<'db>> for ConcreteEnumRef {
    fn from(value: InternedConcreteEnumRef<'db>) -> Self {
        Self(value.0)
    }
}

impl<'db> From<ConcreteEnumRef> for InternedConcreteEnumRef<'db> {
    fn from(value: ConcreteEnumRef) -> Self {
        Self(value.0, PhantomData)
    }
}

impl<'db> From<InternedConcreteStructRef<'db>> for ConcreteStructRef {
    fn from(value: InternedConcreteStructRef<'db>) -> Self {
        Self(value.0)
    }
}

impl<'db> From<ConcreteStructRef> for InternedConcreteStructRef<'db> {
    fn from(value: ConcreteStructRef) -> Self {
        Self(value.0, PhantomData)
    }
}

impl ConcreteEnumRef {
    pub fn interned<'a>(self) -> InternedConcreteEnumRef<'a> {
        self.into()
    }

    pub fn new(db: &dyn Db, def: EnumId, args: Vec<ConcreteTy>) -> Self {
        InternedConcreteEnumRef::new(db, def, args).into()
    }

    pub fn args(self, db: &dyn Db) -> &[ConcreteTy] {
        self.interned().args(db)
    }

    pub fn def(self, db: &dyn Db) -> EnumId {
        self.interned().def(db)
    }

    pub fn into_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeRef::Concrete(TypeId::new(
            db,
            TypeDefId::Enum(self.def(db)),
            self.args(db).iter().map(|a| a.as_type_ref(db)).collect(),
        ))
    }

    pub fn get_cons(self, db: &dyn Db, idx: usize) -> Option<ConstructorType> {
        let as_thir_ref = thir::EnumRef {
            def: self.def(db),
            args: self.args(db).iter().map(|a| a.as_type_ref(db)).collect(),
        };
        as_thir_ref.get_cons(db, idx)
    }
}

impl ConcreteStructRef {
    pub fn interned<'a>(self) -> InternedConcreteStructRef<'a> {
        self.into()
    }

    pub fn new(db: &dyn Db, def: StructId, args: Vec<ConcreteTy>) -> Self {
        InternedConcreteStructRef::new(db, def, args).into()
    }

    pub fn args(self, db: &dyn Db) -> &[ConcreteTy] {
        self.interned().args(db)
    }

    pub fn def(self, db: &dyn Db) -> StructId {
        self.interned().def(db)
    }

    pub fn typeof_field(self, db: &dyn Db, name: Symbol) -> Option<ConcreteTy> {
        self.interned().typeof_field(db, name)
    }

    pub fn into_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeRef::Concrete(TypeId::new(
            db,
            TypeDefId::Struct(self.def(db)),
            self.args(db).iter().map(|a| a.as_type_ref(db)).collect(),
        ))
    }

    pub fn get_fields_ty(self, db: &dyn Db) -> HashMap<Symbol, ConcreteTy> {
        struct_item(db, self.def(db).into())
            .fields
            .iter()
            .map(|f| (f.name, self.typeof_field(db, f.name).unwrap()))
            .collect()
    }
}

#[salsa::tracked]
impl<'db> InternedStructId<'db> {
    #[salsa::tracked(returns(copy))]
    pub fn declared_typeof_field(self, db: &dyn Db, field: Symbol) -> Option<TypeRef> {
        let item = struct_item(db, self);
        let found = item.fields.iter().find(|f| f.name == field)?;
        let ctx = AstImplicitContext::new(
            db,
            ScopeOwnerId::Module(*self.parent(db)),
            templates_of_struct(db, self),
        );
        ctx.resolve(db, &found.ty.data)
    }
}

impl StructId {
    pub fn declared_typeof_field(self, db: &dyn Db, field: Symbol) -> Option<TypeRef> {
        self.interned().declared_typeof_field(db, field)
    }
}

#[salsa::tracked]
impl<'db> InternedConcreteStructRef<'db> {
    #[salsa::tracked(returns(copy))]
    pub fn typeof_field(self, db: &'db dyn Db, field: Symbol) -> Option<ConcreteTy> {
        self.def(db)
            .declared_typeof_field(db, field)?
            .to_concrete(db, &Substitution::new(self.args(db).iter().copied()))
    }
}
