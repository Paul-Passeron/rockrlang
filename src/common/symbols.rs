use std::marker::PhantomData;

#[salsa::interned]
pub struct InternedSymbol {
    pub contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(salsa::Id);

impl<'db> From<InternedSymbol<'db>> for Symbol {
    fn from(s: InternedSymbol<'db>) -> Self {
        Symbol(s.0)
    }
}

impl Symbol {
    pub fn new(db: &dyn crate::Db, s: impl ToString) -> Self {
        Self::from(InternedSymbol::new(db, s.to_string()))
    }

    pub fn interned(&self) -> InternedSymbol<'_> {
        InternedSymbol(self.0, PhantomData)
    }
}

#[salsa::interned]
pub struct InternedStrLit {
    pub contents: String,
}

/// A 'static-compatible StrLit ID, safe to store anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrLit(salsa::Id);

impl<'db> From<InternedStrLit<'db>> for StrLit {
    fn from(s: InternedStrLit<'db>) -> Self {
        StrLit(s.0)
    }
}

impl StrLit {
    pub fn new(db: &dyn crate::Db, s: impl ToString) -> Self {
        Self::from(InternedStrLit::new(db, s.to_string()))
    }

    pub fn interned(&self) -> InternedStrLit<'_> {
        InternedStrLit(self.0, PhantomData)
    }
}
