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
    layout::{IntWidth, Size},
    mir::concrete_ty::{ConcreteTy, InternedConcreteTy},
    name_resolve::builtin_module,
    resolved::{BuiltinTypeId, BuiltinTypeKind, ModuleId, TypeDefId, TypeRef},
    thir_to_mir::{FuncInst, MIRKey},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MangleType {
    Ptr(Box<Self>),
    Tuple(Vec<Self>),
    Array(Box<Self>),
    Bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatKind {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntKind {
    pub width: IntWidth,
    pub signed: bool,
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
    Method { is_static: bool, ty: MangleType, sig: MangleSig, templates: Vec<MangleType> },
    Function { path: Vec<String>, sig: MangleSig, templates: Vec<MangleType> },
}

#[salsa::tracked]
fn _fun_mangle<'db>(db: &'db dyn Db, f: MIRKey<'db>) -> Arc<MangleFun> {
    let fdef = *f.fdef(db);
    let templates =
        f.subs(db).iter().map(|ty| ty_mangle(db, ty).clone()).collect_vec();
    let path = path_of_module(db, owning_module(db, fdef.parent(db)));
    let name = fdef.name(db).to_string(db);
    let inst: FuncInst = f.into();
    let parameters =
        inst.params(db).iter().map(|(_, ty)| ty_mangle(db, *ty).clone()).collect_vec();
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
            let ty = ty_mangle(db, zelf.to_concrete(db, f.subs(db)).unwrap()).clone();
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
pub fn ty_mangle_aux<'db>(db: &'db dyn Db, ty: InternedConcreteTy<'db>) -> MangleType {
    let args = ty.args(db).iter().map(|ty| ty_mangle(db, *ty).clone()).collect_vec();
    let (name, path) = match ty.def(db) {
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

// fn mangle_type_id(db: &dyn Db, id: TypeId) -> MangleType {
//     let args = id.args(db).iter().map(|ty| ty_mangle(db,
// *ty).clone()).collect_vec();     let (name, path) = match id.def(db) {
//         TypeDefId::Builtin(id) => return mangle_builtin_id(db, id, args),
//         TypeDefId::Struct(id) => {
//             let name = id.name(db).to_string(db);
//             let path = path_of_module(db, id.parent(db));
//             (name, path)
//         }
//         TypeDefId::Enum(id) => {
//             let name = id.name(db).to_string(db);
//             let path = path_of_module(db, id.parent(db));
//             (name, path)
//         }
//     };
//     MangleType::Adt { path, name, parameters: args }
// }

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
    match id.kind(db) {
        BuiltinTypeKind::Void => MangleType::Tuple(vec![]),
        BuiltinTypeKind::Never => MangleType::Never,
        BuiltinTypeKind::Bool => MangleType::Bool,
        BuiltinTypeKind::Int { width, signed } => {
            MangleType::Int(IntKind { width, signed })
        }
        BuiltinTypeKind::Ref { .. } | BuiltinTypeKind::Ptr { .. } => {
            MangleType::Ptr(Box::new(args.remove(0)))
        }
        BuiltinTypeKind::Slice => MangleType::Array(Box::new(args.remove(0))),
        BuiltinTypeKind::Tuple => MangleType::Tuple(args),
    }
}

pub fn ty_mangle(db: &dyn Db, ty: ConcreteTy) -> &MangleType {
    ty_mangle_aux(db, ty.interned())
}

pub fn mangle_ident(s: &str) -> String {
    format!("{}{}", s.len(), s)
}

impl IntKind {
    pub fn mangle(self) -> String {
        format!("{}{}", if self.signed { "i" } else { "u" }, {
            let s: Size = self.width.into();
            s.bytes()
        })
    }
}

impl FloatKind {
    pub fn mangle(self) -> String {
        match self {} // uninhabited
    }
}

impl MangleType {
    pub fn mangle(&self) -> String {
        match self {
            Self::Ptr(t) => format!("P{}", t.mangle()),
            Self::Array(t) => format!("A{}", t.mangle()),
            Self::Tuple(ts) => {
                let inner: String = ts.iter().map(Self::mangle).collect();
                format!("T{inner}E")
            }
            Self::Int(k) => k.mangle(),
            Self::Float(k) => format!("f{}", k.mangle()),
            Self::Adt { path, name, parameters } => {
                let idents: String = path
                    .iter()
                    .map(|s| mangle_ident(s))
                    .chain(std::iter::once(mangle_ident(name)))
                    .collect();
                let params: String = parameters.iter().map(Self::mangle).collect();
                format!("N{idents}E{params}E")
            }
            Self::Never => "z".to_owned(),
            Self::Error => "X".to_owned(),
            Self::Bool => "b".to_owned(),
        }
    }
}

impl MangleSig {
    // <name-ident> <param-types...> E <ret-type>
    pub fn mangle(&self) -> String {
        let params: String = self.parameters.iter().map(MangleType::mangle).collect();
        format!("{}{}E{}", mangle_ident(&self.name), params, self.ret.mangle())
    }
}

impl MangleFun {
    pub fn mangle(&self) -> String {
        match self {
            Self::Extern(name) => name.clone(),
            Self::Method { is_static, ty, sig, templates } => {
                let kind = if *is_static { 's' } else { 'm' };
                let tpls: String = templates.iter().map(MangleType::mangle).collect();
                format!("_ZM{kind}{}{}G{tpls}E", ty.mangle(), sig.mangle())
            }
            Self::Function { path, sig, templates } => {
                let p: String = path.iter().map(|s| mangle_ident(s)).collect();
                let tpls: String = templates.iter().map(MangleType::mangle).collect();
                format!("_ZF{p}E{}G{tpls}E", sig.mangle())
            }
        }
    }
}
