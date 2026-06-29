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

use std::ops::Index;

use crate::{
    common::symbols::Symbol,
    layout::LIRTy,
    lir::{branded::InProgressBody, finalized::FunctionBody},
};

pub mod branded;
pub mod finalized;

pub struct ValueData {
    pub name: Symbol,
}

pub struct LIRDef {
    pub ty: LIRTy,
    pub data: ValueData,
}

pub struct Declaring;
pub struct Building;
pub struct Complete;

pub trait ModulePhase {
    type FnBody;
}

impl ModulePhase for Declaring {
    type FnBody = ();
}

impl ModulePhase for Building {
    type FnBody = Option<FunctionBody>;
}

impl ModulePhase for Complete {
    type FnBody = FunctionBody;
}

pub enum DefinedLinkage {
    Export,
    Local,
    Weak,
}

pub enum Body<S: ModulePhase> {
    Import,
    Defined(DefinedLinkage, S::FnBody),
}

pub struct FunctionDecl<S: ModulePhase> {
    pub name: String,
    pub signature: Signature,
    pub body: Body<S>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(usize);

pub struct Signature {
    pub params: Vec<LIRTy>,
    pub ret: LIRTy,
}

pub struct Module<S: ModulePhase> {
    fun_decls: Vec<FunctionDecl<S>>,
}

impl<S: ModulePhase> Index<FunctionId> for Module<S> {
    type Output = FunctionDecl<S>;

    fn index(&self, index: FunctionId) -> &Self::Output {
        &self.fun_decls[index.0]
    }
}

fn build_function<R>(sig: &Signature, f: impl FnOnce(&mut InProgressBody<'_>) -> R) -> R {
    generativity::make_guard!(guard);
    let mut body = InProgressBody::new(guard);
    f(&mut body)
}

impl Module<Declaring> {
    pub fn new() -> Self {
        Self {
            fun_decls: Vec::new(),
        }
    }

    pub fn declare_import(&mut self, name: String, sig: Signature) -> FunctionId {
        let id = FunctionId(self.fun_decls.len());
        self.fun_decls.push(FunctionDecl {
            name,
            signature: sig,
            body: Body::Import,
        });
        id
    }

    pub fn declare_defined(
        &mut self,
        name: String,
        sig: Signature,
        linkage: DefinedLinkage,
    ) -> FunctionId {
        let id = FunctionId(self.fun_decls.len());
        self.fun_decls.push(FunctionDecl {
            name,
            signature: sig,
            body: Body::Defined(linkage, ()),
        });
        id
    }

    pub fn finish_declarations(self) -> Module<Building> {
        let fun_decls = self
            .fun_decls
            .into_iter()
            .map(|fun_decl| FunctionDecl {
                name: fun_decl.name,
                signature: fun_decl.signature,
                body: match fun_decl.body {
                    Body::Import => Body::Import,
                    Body::Defined(defined_linkage, _) => {
                        Body::Defined(defined_linkage, None)
                    }
                },
            })
            .collect();
        Module { fun_decls }
    }
}

