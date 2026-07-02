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

use std::marker::PhantomData;

use crate::{
    common::arena::Idx,
    layout::{LIRTy, ScalarKind},
    lir::{
        branded::{BrandedBlockId, Invariant},
        finalized::{BlockData, FunctionBody},
    },
};

pub mod branded;
pub mod build;
pub mod finalized;
pub mod inst;

pub struct LIRDef {
    pub ty: LIRTy,
}

pub trait ValueKind: Copy + 'static {
    fn matches(class: ValueClass) -> bool;
}
pub trait ScalarMarker: Copy + 'static {}

#[derive(Clone, Copy)]
pub struct Scalar<S: ScalarMarker>(PhantomData<S>);

#[derive(Clone, Copy)]
pub struct Int;

#[derive(Clone, Copy)]
pub struct Ptr<P: ValueKind>(PhantomData<P>);

#[derive(Clone, Copy)]
pub struct Aggregate;

#[derive(Clone, Copy)]
pub struct Union;

#[derive(Clone, Copy)]
pub struct ZST;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueClass {
    Scalar(ScalarKind),
    Aggregate,
    Union,
    None,
}

impl ScalarMarker for Int {}
impl<S: ScalarMarker> ValueKind for Scalar<S> {
    fn matches(class: ValueClass) -> bool {
        matches!(class, ValueClass::Scalar(ScalarKind::Int(_)))
    }
}

impl<P: ValueKind> ValueKind for Ptr<P> {
    fn matches(class: ValueClass) -> bool {
        matches!(class, ValueClass::Scalar(ScalarKind::Ptr))
    }
}

impl<P: ValueKind> ScalarMarker for Ptr<P> {}

impl ValueKind for Aggregate {
    fn matches(class: ValueClass) -> bool {
        matches!(class, ValueClass::Aggregate)
    }
}

impl ValueKind for Union {
    fn matches(class: ValueClass) -> bool {
        matches!(class, ValueClass::Union)
    }
}

impl ValueKind for ZST {
    fn matches(class: ValueClass) -> bool {
        class == ValueClass::None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId<'ir> {
    idx: Idx<LIRDef>,
    _brand: PhantomData<Invariant<'ir>>,
}

pub struct ValueDef<'ir> {
    idx: Idx<LIRDef>,
    _brand: PhantomData<Invariant<'ir>>,
}

impl<'ir> ValueDef<'ir> {
    pub fn id(&self) -> ValueId<'ir> {
        ValueId { idx: self.idx, _brand: PhantomData }
    }
}

pub struct Typed<'ir, K: ValueKind> {
    id: ValueId<'ir>,
    ty: LIRTy,
    _k: PhantomData<K>,
}

impl<'ir, K: ValueKind> Typed<'ir, K> {
    pub fn erase(self) -> ValueId<'ir> {
        self.id
    }
}

pub type ScalarValue<'ir, S> = Typed<'ir, Scalar<S>>;
pub type IntValue<'ir> = Typed<'ir, Scalar<Int>>;
pub type AggregateValue<'ir> = Typed<'ir, Aggregate>;
pub type UnionValue<'ir> = Typed<'ir, Union>;

#[derive(Clone, Copy)]
pub struct TypedPtr<'ir, K: ValueKind> {
    raw: ValueId<'ir>,
    pointee: LIRTy,
    _k: PhantomData<K>,
}

pub trait Refs {
    type Val: Copy; // use of value
    type Def;
    type Block: Copy; // use of a block
}

pub struct Branded<'ir>(PhantomData<Invariant<'ir>>);
impl<'ir> Refs for Branded<'ir> {
    type Val = ValueId<'ir>;
    type Def = ValueDef<'ir>;
    type Block = BrandedBlockId<'ir>;
}

pub struct Finalized;
impl Refs for Finalized {
    type Val = Idx<LIRDef>;
    type Def = Idx<LIRDef>;
    type Block = Idx<BlockData>;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefinedLinkage {
    Export,
    Local,
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArithBinop {
    UAdd,
    USub,
    UMul,
    UDiv,
    UMod,
    SAdd,
    SSub,
    SMul,
    SDiv,
    SMod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CmpBinop {
    SLessThan,
    ULessThan,
    Eq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Logic {
    Or,
    And,
}

pub enum Body<S: ModulePhase> {
    Import,
    Defined(DefinedLinkage, S::FnBody),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LIRFunctionId(usize);

pub struct Signature {
    pub params: Vec<LIRTy>,
    pub ret: LIRTy,
}

pub struct FunctionSig {
    pub name: String,
    pub signature: Signature,
    pub kind: SigKind,
}

pub enum SigKind {
    Import,
    Defined(DefinedLinkage),
}

pub struct Module<S: ModulePhase> {
    sigs: Vec<FunctionSig>,
    bodies: Vec<Body<S>>,
}

pub enum VerifyError {}

impl<S: ModulePhase> Module<S> {
    pub fn get_fn(&self, id: LIRFunctionId) -> (&FunctionSig, &Body<S>) {
        (&self.sigs[id.0], &self.bodies[id.0])
    }

    pub fn get_fn_mut(
        &mut self,
        id: LIRFunctionId,
    ) -> (&FunctionSig, &mut Body<S>) {
        (&self.sigs[id.0], &mut self.bodies[id.0])
    }
}

impl Module<Declaring> {
    pub fn new() -> Self {
        Self { sigs: Vec::new(), bodies: Vec::new() }
    }

    pub fn declare_import(
        &mut self,
        name: String,
        sig: Signature,
    ) -> LIRFunctionId {
        let id = LIRFunctionId(self.sigs.len());
        self.bodies.push(Body::Import);
        self.sigs.push(FunctionSig {
            name,
            signature: sig,
            kind: SigKind::Import,
        });
        id
    }

    pub fn declare_defined(
        &mut self,
        name: String,
        sig: Signature,
        linkage: DefinedLinkage,
    ) -> LIRFunctionId {
        let id = LIRFunctionId(self.sigs.len());
        self.bodies.push(Body::Defined(linkage, ()));
        self.sigs.push(FunctionSig {
            name,
            signature: sig,
            kind: SigKind::Defined(linkage),
        });
        id
    }

    pub fn finish_declarations(self) -> Module<Building> {
        Module {
            sigs: self.sigs,
            bodies: self
                .bodies
                .into_iter()
                .map(|body| body.map(|_| None))
                .collect(),
        }
    }
}

impl Module<Building> {
    pub fn finalize(self) -> Module<Complete> {
        todo!()
    }
}

impl<S: ModulePhase> Body<S> {
    pub fn map<NewPhase: ModulePhase>(
        self,
        f: impl FnOnce(S::FnBody) -> NewPhase::FnBody,
    ) -> Body<NewPhase> {
        match self {
            Body::Import => Body::Import,
            Body::Defined(defined_linkage, val) => {
                Body::Defined(defined_linkage, f(val))
            }
        }
    }
}
