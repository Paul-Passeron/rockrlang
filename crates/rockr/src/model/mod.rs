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

use crate::model::type_printer::TypePrinter;
use crate::name_resolve::type_expr::{templates_of_enum, templates_of_struct};
use crate::typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext};
pub use builtin::*;
use rockr_base::{
    Db, SourceFile, Span, lang::arch::IntWidth, mutability::Mutability, symbol::Symbol,
    unord::Set,
};
use rockr_syntax::parse_tree::top_level::AstEnumVariantKind;
use rockr_syntax::parse_tree::top_level::{AstImplItem, AstTemplateArg};
use std::collections::HashMap;

pub mod builtin;
pub mod concrete_ty;
pub mod packages;
pub mod type_printer;

#[salsa::tracked]
#[derive(PartialOrd, Ord)]
pub struct FileModule<'db> {
    pub file: SourceFile,
    pub submodules: Vec<FileModule<'db>>,
}

impl<'db> FileModule<'db> {
    pub fn name(self, db: &'db dyn Db) -> Symbol {
        let path = self.file(db).path(db);
        let stem = path
            .parent()
            .and_then(|parent| {
                // If this file is main.rkr, the module name is the directory
                // name
                if path.file_name().and_then(|n| n.to_str()) == Some("main.rkr") {
                    parent.file_name()
                } else {
                    None
                }
            })
            .or_else(|| path.file_stem())
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Symbol::new(db, stem)
    }
}

#[salsa::tracked]
#[derive(PartialOrd, Ord)]
pub struct Package<'db> {
    pub root: FileModule<'db>,
}

#[salsa::interned]
pub struct InternedModuleId<'db> {
    pub name: Symbol,
    pub parent: Option<ModuleId>,
    pub file: Option<SourceFile>,
    pub file_submodules: Vec<FileModule<'db>>,
    pub package: Option<Package<'db>>,
}

#[salsa::interned]
pub struct InternedFunctionId {
    pub name: Symbol,
    pub parent: ScopeOwnerId,
}

#[salsa::interned]
pub struct InternedStructId {
    pub name: Symbol,
    pub parent: ModuleId,
}

#[salsa::interned]
pub struct InternedEnumId {
    pub name: Symbol,
    pub parent: ModuleId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeParamId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct TypeParam {
    pub name: Symbol,
    pub constraints: Vec<InterfaceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeRef {
    Concrete(TypeId),
    Param(TypeParamId),
    Associated(Symbol),
    Zelf,
    Error,
    Unknown,
}

#[salsa::interned]
pub struct InternedImplId {
    pub parent: ModuleId,
    pub implemented: TypeRef,
    pub interface: Option<InterfaceRef>,
    pub templates: Vec<Set<InterfaceRef>>,
}

#[salsa::interned]
pub struct InternedInterfaceId {
    pub name: Symbol,
    pub parent: ModuleId,
}

#[salsa::interned]
pub struct InternedInterfaceRef {
    pub def: InterfaceId,
    pub args: Vec<TypeRef>,
}

#[salsa::interned]
pub struct InternedTypeId {
    pub def: TypeDefId,
    pub args: Vec<TypeRef>, // None means inferred
}

#[salsa::interned]
pub struct BuiltinTypeDef {
    pub kind: BuiltinTypeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntWidthKind {
    System, // For isize / usize
    Fixed(IntWidth),
}

impl From<IntWidth> for IntWidthKind {
    fn from(value: IntWidth) -> Self {
        Self::Fixed(value)
    }
}

impl IntWidthKind {
    pub fn as_suffix(self) -> &'static str {
        match self {
            Self::System => "size",
            Self::Fixed(IntWidth::I8) => "8",
            Self::Fixed(IntWidth::I16) => "16",
            Self::Fixed(IntWidth::I32) => "32",
            Self::Fixed(IntWidth::I64) => "64",
            Self::Fixed(IntWidth::I128) => "128",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinTypeKind {
    Void,
    Never,

    Bool,
    Int { width: IntWidthKind, signed: bool },

    Ref { mutability: Mutability },
    Ptr { mutability: Mutability },
    Slice { mutability: Mutability },

    Array { length: usize },
    Tuple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeOwnerId {
    Module(ModuleId),
    Impl(ImplId),
    Interface(InterfaceRef),
}

impl From<ImplId> for ScopeOwnerId {
    fn from(value: ImplId) -> Self {
        Self::Impl(value)
    }
}

impl From<ModuleId> for ScopeOwnerId {
    fn from(value: ModuleId) -> Self {
        Self::Module(value)
    }
}

impl From<InterfaceRef> for ScopeOwnerId {
    fn from(value: InterfaceRef) -> Self {
        Self::Interface(value)
    }
}

impl ScopeOwnerId {
    pub fn zelf_or_error(&self, db: &dyn Db) -> TypeRef {
        self.get_canonical_zelf(db).unwrap_or(TypeRef::Error)
    }

    pub fn get_canonical_zelf(&self, db: &dyn Db) -> Option<TypeRef> {
        match self {
            Self::Module(_) => None,
            Self::Impl(impl_id) => Some(impl_id.implemented(db)),
            Self::Interface(_) => Some(TypeRef::Zelf),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeDefId {
    Builtin(BuiltinTypeId),
    Struct(StructId),
    Enum(EnumId),
}

impl TypeDefId {
    pub fn to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().type_def_id_to_string(db, self)
    }

    pub fn is_int_like(self, db: &dyn Db) -> Option<BuiltinTypeId> {
        match self {
            Self::Builtin(b) => b.is_int_like(db),
            _ => None,
        }
    }
}

#[salsa::tracked]
#[derive(Debug, PartialOrd, Ord)]
pub struct ImplSource<'db> {
    pub id: ImplId,
    pub module: ModuleId,
    pub templates: Vec<AstTemplateArg>,
    pub items: Vec<AstImplItem>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionId(salsa::Id);

impl FunctionId {
    pub fn sig_to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().function_id_to_string(db, self)
    }

    pub fn called_to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().called_function_to_string(db, self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImplId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceId(salsa::Id);

impl InterfaceId {
    pub fn to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().interface_id_to_string(db, self)
    }
}

impl InterfaceRef {
    pub fn to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().interface_ref_to_string(db, self)
    }
}

impl ImplId {
    pub fn to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().impl_id_to_string(db, self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuiltinTypeId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceRef(salsa::Id);

impl BuiltinTypeId {
    pub fn is_signed(self, db: &dyn Db) -> bool {
        match self.kind(db) {
            BuiltinTypeKind::Int { signed, .. } => signed,
            _ => false,
        }
    }
}

pub fn templates_of_type_def(db: &dyn Db, ty: TypeDefId) -> usize {
    match ty {
        TypeDefId::Builtin(id) => id.template_count(db),
        TypeDefId::Struct(id) => templates_of_struct(db, id.into()).len(),
        TypeDefId::Enum(id) => templates_of_enum(db, id.into()).len(),
    }
}

pub fn merge_type_ref(db: &dyn Db, a: TypeRef, b: TypeRef) -> TypeRef {
    match (a, b) {
        (TypeRef::Param(a), TypeRef::Param(b)) => {
            if a != b {
                return TypeRef::Error;
            }
            TypeRef::Param(a)
        }
        (TypeRef::Param(a), _) | (_, TypeRef::Param(a)) => TypeRef::Param(a),
        (TypeRef::Zelf, _) | (_, TypeRef::Zelf) => TypeRef::Zelf,
        (TypeRef::Error | TypeRef::Unknown, other)
        | (other, TypeRef::Error | TypeRef::Unknown) => other,
        (TypeRef::Associated(s), _) | (_, TypeRef::Associated(s)) => {
            TypeRef::Associated(s)
        }
        (TypeRef::Concrete(a), TypeRef::Concrete(b)) => {
            let a_def = a.def(db);
            if a.def(db) != b.def(db) {
                return TypeRef::Error;
            }
            let a_args = a.args(db);
            let b_args = b.args(db);
            if a_args.len() != b_args.len() {
                return TypeRef::Error;
            }

            let args = a_args
                .iter()
                .zip(b_args)
                .map(|(a, b)| merge_type_ref(db, *a, *b))
                .collect();

            TypeId::new(db, a_def, args).into()
        }
    }
}

pub fn rehole(db: &dyn Db, ty: TypeRef) -> TypeRef {
    let Some(id) = ty.as_type_id() else {
        return ty;
    };
    let def = id.def(db);
    let n = templates_of_type_def(db, def);
    let args = id.args(db);
    let args = (0..n)
        .into_iter()
        .map(|i| args.get(i).copied().unwrap_or(TypeRef::Unknown))
        .collect();

    TypeId::new(db, def, args).into()
}

pub fn compute_template_hints(
    db: &dyn Db,
    ty: TypeRef,
    type_args: Vec<TypeRef>,
) -> Vec<TypeRef> {
    let Some(reholed) = rehole(db, ty).as_type_id() else {
        return type_args;
    };

    reholed
        .args(db)
        .iter()
        .zip(type_args)
        .map(|(a, b)| merge_type_ref(db, *a, b))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastClass {
    Int,
    ThinPtr(Mutability),
    FatPtr(Mutability),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Subst<'a> {
    pub params: Option<&'a [TypeRef]>,
    pub zelf: Option<TypeRef>,
}

impl<'a> Subst<'a> {
    pub fn params(params: &'a [TypeRef]) -> Self {
        Self { params: Some(params), zelf: None }
    }

    pub fn zelf(zelf: TypeRef) -> Self {
        Self { params: None, zelf: Some(zelf) }
    }

    #[must_use]
    pub fn with_zelf(self, zelf: Option<TypeRef>) -> Self {
        Self { zelf, ..self }
    }
}

impl TypeRef {
    pub fn to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().type_ref_to_string(db, self)
    }

    pub fn as_type_id(self) -> Option<TypeId> {
        match self {
            Self::Concrete(type_id) => Some(type_id),
            _ => None,
        }
    }
}

impl TryFrom<TypeRef> for TypeId {
    type Error = ();

    fn try_from(value: TypeRef) -> Result<Self, Self::Error> {
        value.as_type_id().ok_or(())
    }
}

pub trait TyLike: Sized {
    type Sub<'a>
    where
        Self: 'a;

    fn as_builtin<'a>(&'a self, db: &'a dyn Db) -> Option<(BuiltinTypeId, &'a [Self])>;

    fn sub(ty: &Self) -> Self::Sub<'_>;

    fn as_ptr_like<'a>(&'a self, db: &'a dyn Db) -> Option<(Mutability, Self::Sub<'a>)> {
        let (kind, inner) = ptr_like(self, db)?;
        Some((kind.mutability(), Self::sub(inner)))
    }

    fn as_ref<'a>(&'a self, db: &'a dyn Db) -> Option<(Mutability, Self::Sub<'a>)> {
        match ptr_like(self, db)? {
            (PtrKind::Ref(mutability), inner) => Some((mutability, Self::sub(inner))),
            (PtrKind::RawPtr(_), _) => None,
        }
    }

    fn as_ptr<'a>(&'a self, db: &'a dyn Db) -> Option<(Mutability, Self::Sub<'a>)> {
        match ptr_like(self, db)? {
            (PtrKind::RawPtr(mutability), inner) => Some((mutability, Self::sub(inner))),
            (PtrKind::Ref(_), _) => None,
        }
    }

    fn as_array<'a>(&'a self, db: &'a dyn Db) -> Option<(Self::Sub<'a>, usize)> {
        let (b, args) = self.as_builtin(db)?;
        match b.kind(db) {
            BuiltinTypeKind::Array { length } => Some((Self::sub(&args[0]), length)),
            _ => None,
        }
    }

    fn as_slice<'a>(&'a self, db: &'a dyn Db) -> Option<(Self::Sub<'a>, Mutability)> {
        let (b, args) = self.as_builtin(db)?;
        match b.kind(db) {
            BuiltinTypeKind::Slice { mutability } => {
                Some((Self::sub(&args[0]), mutability))
            }
            _ => None,
        }
    }

    fn as_tuple<'a>(&'a self, db: &'a dyn Db) -> Option<&'a [Self]> {
        let (b, args) = self.as_builtin(db)?;
        matches!(b.kind(db), BuiltinTypeKind::Tuple).then_some(args)
    }
}

fn ptr_like<'a, T: TyLike>(ty: &'a T, db: &'a dyn Db) -> Option<(PtrKind, &'a T)> {
    let (b, args) = ty.as_builtin(db)?;
    Some((b.is_ptr_like(db)?, &args[0]))
}

impl TyLike for TypeRef {
    type Sub<'a> = Self;

    fn as_builtin<'a>(&'a self, db: &'a dyn Db) -> Option<(BuiltinTypeId, &'a [Self])> {
        let id = self.as_type_id()?;
        match id.def(db) {
            TypeDefId::Builtin(bid) => Some((bid, id.args(db))),
            _ => None,
        }
    }

    fn sub(ty: &Self) -> Self {
        *ty
    }
}

impl TypeRef {
    pub fn cast_class(self, db: &dyn Db) -> Option<CastClass> {
        if let Some((muta, _)) = self.as_ptr_like(db) {
            return Some(if self.is_fat_ptr(db) {
                CastClass::FatPtr(muta)
            } else {
                CastClass::ThinPtr(muta)
            });
        }
        let (builtin, _) = self.as_builtin(db)?;
        matches!(builtin.kind(db), BuiltinTypeKind::Bool | BuiltinTypeKind::Int { .. })
            .then_some(CastClass::Int)
    }

    pub fn element_of_indexed(self, db: &dyn Db) -> Option<Self> {
        self.as_ptr(db)
            .map(|(_, inner)| inner)
            .or_else(|| self.as_array(db).map(|ty| ty.0))
            .or_else(|| self.as_slice(db).map(|ty| ty.0))
            .or_else(|| self.as_ref(db).and_then(|t| t.1.as_array(db).map(|ty| ty.0)))
    }

    // TODO: What was the point of this, exactly ???
    pub fn ref_slice_of(db: &dyn Db, inner: Self) -> Self {
        Self::Concrete(slice_of(db, inner, Mutability::Const))
    }

    pub fn typeof_metadata(&self, db: &dyn Db) -> Option<Self> {
        self.is_fat_ptr(db).then(|| usize_id(db).as_type_ref(db))
    }

    #[must_use]
    pub fn wrap_ref(self, db: &dyn Db, mutable: bool) -> Self {
        match self {
            Self::Error | Self::Unknown => self,
            _ => Self::Concrete(ref_of(db, self, mutable)),
        }
    }

    #[must_use]
    pub fn subst(self, db: &dyn Db, subst: &Subst<'_>) -> Self {
        match self {
            Self::Concrete(id) => Self::Concrete(TypeId::new(
                db,
                id.def(db),
                id.args(db).iter().map(|t| t.subst(db, subst)).collect(),
            )),
            Self::Param(p) => match subst.params {
                Some(params) => params.get(p.0).copied().unwrap_or(Self::Error),
                None => self,
            },
            Self::Zelf => subst.zelf.unwrap_or(self),
            Self::Associated(_) | Self::Error | Self::Unknown => self,
        }
    }
}

pub enum ConstructorType {
    Tuple(Vec<TypeRef>),
    Struct(Vec<(Symbol, TypeRef)>),
    None,
}

impl ConstructorType {
    #[must_use]
    pub fn with_substitution(&self, db: &dyn Db, sub: &[TypeRef]) -> Self {
        match self {
            Self::Tuple(type_refs) => Self::Tuple(
                type_refs.iter().map(|ty| ty.subst(db, &Subst::params(sub))).collect(),
            ),
            Self::Struct(items) => Self::Struct(
                items
                    .iter()
                    .map(|(name, ty)| (*name, ty.subst(db, &Subst::params(sub))))
                    .collect(),
            ),
            Self::None => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapKind {
    Ref(Mutability),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefWrappedTy<T: AdtLike> {
    pub inner: T,
    pub refs: Vec<WrapKind>, // [&A, &B, &C] means &C &B &A T
}

impl<T: AdtLike> RefWrappedTy<T> {
    pub fn wrap_like(&self, db: &dyn Db, to_wrap: T) -> T {
        self.refs.iter().fold(to_wrap, |ty, ref_kind| ref_kind.wrap(db, ty.into()))
    }

    pub fn as_type_ref(&self, db: &dyn Db) -> T {
        self.wrap_like(db, self.inner)
    }

    pub fn peel_until(db: &dyn Db, to_peel: T, target: T) -> Option<Self> {
        let mut refs = vec![];
        let mut inner = to_peel;
        while inner != target
            && let Some((mutability, ty)) = inner.peel_ref(db)
        {
            inner = ty.try_into().ok()?;
            refs.push(WrapKind::Ref(mutability));
        }
        if inner != target {
            return None;
        }
        Some(Self { inner, refs })
    }

    pub fn from_type(db: &dyn Db, ty: T) -> Self {
        let mut refs = vec![];
        let mut inner = ty;
        while let Some((mutability, ty)) = inner.peel_ref(db) {
            inner = ty.try_into().ok().unwrap();
            refs.push(WrapKind::Ref(mutability));
        }
        refs.reverse();
        Self { inner, refs }
    }

    pub fn depth(&self) -> usize {
        self.refs.len()
    }
}

impl WrapKind {
    pub fn wrap<T: AdtLike>(self, db: &dyn Db, ty: T::Args) -> T {
        let Self::Ref(mutability) = self;
        ref_of(db, ty, mutability.is_mut())
    }
}

impl TypeRef {
    pub fn contains_error(&self, db: &dyn Db) -> bool {
        match self {
            Self::Error => true,
            Self::Concrete(id) => id.args(db).iter().any(|arg| arg.contains_error(db)),
            Self::Param(_) | Self::Associated(_) | Self::Zelf | Self::Unknown => false,
        }
    }
}

/// Applies `refs` (as collected by [`peel_type_ref_until`]) on top of `ty`.
///
/// Unlike [`RefWrappedTy::wrap_like`], this works directly on `TypeRef` and so
/// tolerates non-concrete leaves (e.g. a bare `Param` inside a generic
/// function's own body), which `RefWrappedTy<TypeId>` cannot represent.
pub fn wrap_type_ref(db: &dyn Db, refs: &[WrapKind], ty: TypeRef) -> TypeRef {
    refs.iter()
        .fold(ty, |ty, WrapKind::Ref(mutability)| ty.wrap_ref(db, mutability.is_mut()))
}

/// Like [`RefWrappedTy`], but over `TypeRef` directly — `AdtLike` cannot be
/// implemented for `TypeRef` since a bare `Param`/`Zelf`/`Error`/`Unknown` has
/// no `def()`, and those legitimately appear here in unmonomorphized THIR.
pub struct PeeledTy {
    pub inner: TypeRef,
    pub refs: Vec<WrapKind>,
}

impl PeeledTy {
    pub fn from_type_ref(db: &dyn Db, mut ty: TypeRef) -> Self {
        let mut refs = vec![];
        while let Some((mutability, inner)) = ty.as_ref(db) {
            ty = inner;
            refs.push(WrapKind::Ref(mutability));
        }
        refs.reverse();
        Self { inner: ty, refs }
    }
}

pub fn peel_type_ref_until(
    db: &dyn Db,
    mut ty: TypeRef,
    target: TypeRef,
) -> Option<PeeledTy> {
    let mut refs = vec![];
    while ty != target
        && let Some((mutability, inner)) = ty.as_ref(db)
    {
        ty = inner;
        refs.push(WrapKind::Ref(mutability));
    }
    if ty != target {
        return None;
    }
    Some(PeeledTy { inner: ty, refs })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructRef {
    pub def: StructId,
    pub args: Vec<TypeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumRef {
    pub def: EnumId,
    pub args: Vec<TypeRef>,
}

impl EnumRef {
    pub fn into_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Enum(self.def), self.args).into()
    }
}

impl StructRef {
    pub fn into_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Struct(self.def), self.args).into()
    }
}

impl EnumRef {
    pub fn get_cons(&self, db: &dyn Db, idx: usize) -> Option<ConstructorType> {
        let item = self.def.item(db);
        let variant = item.variants.get(idx)?;
        let raw = match &variant.kind {
            AstEnumVariantKind::Unit => Some(ConstructorType::None),
            AstEnumVariantKind::StructLike(fields) => {
                let ctx = AstImplicitContext::new(
                    db,
                    ScopeOwnerId::Module(self.def.parent(db)),
                    &item.template_args,
                );
                Some(ConstructorType::Struct(
                    fields
                        .iter()
                        .map(|field| (field.name, ctx.resolve_err(db, &field.ty.data)))
                        .collect(),
                ))
            }
            AstEnumVariantKind::TupleLike(spanneds) => {
                let ctx = AstImplicitContext::new(
                    db,
                    ScopeOwnerId::Module(self.def.parent(db)),
                    &item.template_args,
                );
                Some(ConstructorType::Tuple(
                    spanneds.iter().map(|ast| ctx.resolve_err(db, &ast.data)).collect(),
                ))
            }
        }?;
        Some(raw.with_substitution(db, &self.args))
    }

    pub fn variants(&self, db: &dyn Db) -> Vec<ConstructorType> {
        let n = self.def.item(db).variants.len();
        (0..n).into_iter().map(|i| self.get_cons(db, i).unwrap()).collect()
    }
}

impl StructRef {
    /// Field types as declared on the struct definition, still referencing the
    /// struct's own template parameters rather than the concrete `self.args`.
    pub fn declared_fields_ty(&self, db: &dyn Db) -> HashMap<Symbol, TypeRef> {
        let item = self.def.item(db);
        let mut res = HashMap::new();
        for field in &item.fields {
            let ctx = AstImplicitContext::new(
                db,
                ScopeOwnerId::Module(self.def.parent(db)),
                &item.template_args,
            );
            res.insert(field.name, ctx.resolve_err(db, &field.ty.data));
        }
        res
    }

    pub fn get_fields_ty(&self, db: &dyn Db) -> HashMap<Symbol, TypeRef> {
        self.declared_fields_ty(db)
            .into_iter()
            .map(|(name, ty)| (name, ty.subst(db, &Subst::params(&self.args))))
            .collect()
    }
}
