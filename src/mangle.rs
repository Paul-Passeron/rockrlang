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

use itertools::Itertools;

use crate::{
    Db,
    hir::{FunctionLikeAst, function_ast, owning_module},
    name_resolve::builtin_module,
    ril::{BuiltinTypeId, ModuleId, TypeDefId, TypeId, TypeRef},
    thir_to_mir::{FuncInst, MIRKey},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MangleType {
    Ptr(Box<Self>),
    Tuple(Vec<Self>),
    Array(Box<Self>),
    Int(IntKind), // width in bytes
    #[allow(unused)]
    Float(FloatKind), // width in bytes
    Adt {
        path: Vec<String>,
        name: String,
        parameters: Vec<Self>,
    },
    Never,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FloatKind {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntKind {
    Bool,
    Int,
    Usize,
    Char,
    // ...
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MangleSig {
    name: String,
    parameters: Vec<MangleType>,
    ret: MangleType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MangleFun {
    Extern(String), // An extern function should not be mangled
    Method {
        is_static: bool,
        ty: MangleType,
        sig: MangleSig,
        templates: Vec<MangleType>,
    },
    Function {
        path: Vec<String>,
        sig: MangleSig,
        templates: Vec<MangleType>,
    },
}

#[salsa::tracked]
fn _fun_mangle<'db>(db: &'db dyn Db, f: MIRKey<'db>) -> Arc<MangleFun> {
    let fdef = *f.fdef(db);
    let templates =
        f.subs(db).iter().map(|ty| ty_mangle(db, *ty).clone()).collect_vec();
    let path = path_of_module(db, owning_module(db, fdef.parent(db)));
    let name = fdef.name(db).to_string(db);
    let inst: FuncInst = f.into();
    let parameters = inst
        .params(db)
        .iter()
        .map(|(_, ty)| ty_mangle(db, *ty).clone())
        .collect_vec();
    let ret = ty_mangle(db, inst.ret_ty(db)).clone();
    let sig = MangleSig { name, parameters, ret };
    match function_ast(db, fdef.into()).inner(db) {
        FunctionLikeAst::ExternDef(_, _) => {
            Arc::new(MangleFun::Extern(fdef.name(db).to_string(db)))
        }
        FunctionLikeAst::Fundef(_) => {
            Arc::new(MangleFun::Function { path, sig, templates })
        }
        FunctionLikeAst::Method(method) => {
            let zelf = fdef.parent(db).get_canonical_zelf(db).unwrap();
            let ty = ty_mangle(db, zelf).clone();
            let is_static = method.data.receiver.is_static();
            Arc::new(MangleFun::Method { is_static, ty, sig, templates })
        }
        FunctionLikeAst::TraitMethod(_) => todo!(),
    }
}

pub fn fun_mangle(db: &dyn Db, f: FuncInst) -> &MangleFun {
    _fun_mangle(db, f.interned())
}

#[salsa::interned]
struct InternedTR {
    pub tref: TypeRef,
}

#[salsa::tracked]
pub fn _ty_mangle<'db>(db: &'db dyn Db, ty: InternedTR<'db>) -> MangleType {
    
    match ty.tref(db) {
        TypeRef::Concrete(type_id) => mangle_type_id(db, *type_id),
        _ => MangleType::Error,
    }
}

fn mangle_type_id(db: &dyn Db, id: TypeId) -> MangleType {
    let args =
        id.args(db).iter().map(|ty| ty_mangle(db, *ty).clone()).collect_vec();
    let (name, path) = match id.def(db) {
        TypeDefId::Builtin(id) => return mangle_builtin_id(db, id, args),
        TypeDefId::Struct(id) => {
            let name = id.name(db).to_string(db);
            let path = path_of_module(db, id.parent(db));
            (name, path)
        }
        TypeDefId::Enum(id) => {
            let name = id.name(db).to_string(db);
            let path = path_of_module(db, id.parent(db));
            (name, path)
        }
    };
    MangleType::Adt { path, name, parameters: args }
}

fn path_of_module(db: &dyn Db, m: ModuleId) -> Vec<String> {
    fn aux(db: &dyn Db, m: ModuleId, res: &mut Vec<String>) {
        if m == builtin_module(db) {
            return;
        }
        res.push(m.name(db).to_string(db));
        if let Some(m) = m.parent(db) {
            aux(db, m, res);
        }
    }
    let mut res = vec![];
    aux(db, m, &mut res);
    res.reverse();
    res
}

fn mangle_builtin_id(
    db: &dyn Db,
    id: BuiltinTypeId,
    mut args: Vec<MangleType>,
) -> MangleType {
    if id.is_ptr_like(db).is_some() {
        MangleType::Ptr(Box::new(args.remove(0)))
    } else if let Some(int_kind) = id.as_int_kind(db) {
        MangleType::Int(int_kind)
    } else if id == BuiltinTypeId::slice(db) {
        MangleType::Array(Box::new(args.remove(0)))
    } else if id == BuiltinTypeId::tuple(db) {
        MangleType::Tuple(args)
    } else if id == BuiltinTypeId::never(db) {
        MangleType::Never
    } else if id == BuiltinTypeId::void(db) {
        MangleType::Tuple(vec![])
    } else {
        todo!("mangle {}", id.name(db).to_string(db))
    }
}

pub fn ty_mangle(db: &dyn Db, tref: TypeRef) -> &MangleType {
    _ty_mangle(db, InternedTR::new(db, tref))
}

impl BuiltinTypeId {
    fn as_int_kind(self, db: &dyn Db) -> Option<IntKind> {
        if self == BuiltinTypeId::int(db) {
            Some(IntKind::Int)
        } else if self == BuiltinTypeId::bool(db) {
            Some(IntKind::Bool)
        } else if self == BuiltinTypeId::char(db) {
            Some(IntKind::Char)
        } else if self == BuiltinTypeId::usize(db) {
            Some(IntKind::Usize)
        } else {
            None
        }
    }
}

pub fn mangle_ident(s: &str) -> String {
    format!("{}{}", s.len(), s)
}

impl IntKind {
    pub fn mangle(&self) -> &'static str {
        match self {
            IntKind::Bool => "ib",
            IntKind::Int => "ii",
            IntKind::Usize => "iu",
            IntKind::Char => "ic",
            // ...
        }
    }
}

impl FloatKind {
    pub fn mangle(&self) -> String {
        match *self {} // uninhabited
    }
}

impl MangleType {
    pub fn mangle(&self) -> String {
        match self {
            MangleType::Ptr(t) => format!("P{}", t.mangle()),
            MangleType::Array(t) => format!("A{}", t.mangle()),
            MangleType::Tuple(ts) => {
                let inner: String = ts.iter().map(Self::mangle).collect();
                format!("T{inner}E")
            }
            MangleType::Int(k) => k.mangle().to_string(),
            MangleType::Float(k) => format!("f{}", k.mangle()),
            MangleType::Adt { path, name, parameters } => {
                let idents: String = path
                    .iter()
                    .map(|s| mangle_ident(s))
                    .chain(std::iter::once(mangle_ident(name)))
                    .collect();
                let params: String =
                    parameters.iter().map(Self::mangle).collect();
                format!("N{idents}E{params}E")
            }
            MangleType::Never => "z".to_string(),
            MangleType::Error => "X".to_string(),
        }
    }
}

impl MangleSig {
    // <name-ident> <param-types...> E <ret-type>
    pub fn mangle(&self) -> String {
        let params: String =
            self.parameters.iter().map(MangleType::mangle).collect();
        format!("{}{}E{}", mangle_ident(&self.name), params, self.ret.mangle())
    }
}

impl MangleFun {
    pub fn mangle(&self) -> String {
        match self {
            MangleFun::Extern(name) => name.clone(),
            MangleFun::Method { is_static, ty, sig, templates } => {
                let kind = if *is_static { 's' } else { 'm' };
                let tpls: String =
                    templates.iter().map(MangleType::mangle).collect();
                format!("_ZM{kind}{}{}G{tpls}E", ty.mangle(), sig.mangle())
            }
            MangleFun::Function { path, sig, templates } => {
                let p: String = path.iter().map(|s| mangle_ident(s)).collect();
                let tpls: String =
                    templates.iter().map(MangleType::mangle).collect();
                format!("_ZF{p}E{}G{tpls}E", sig.mangle())
            }
        }
    }
}
