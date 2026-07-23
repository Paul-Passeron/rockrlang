# TODO for Rockr

This can be a great starting point for contributors looking for something to do.

## Short-term

- [ ] Better diagnostics
  - [x] Display diagnostics
  - [ ] Handle constraint solver output gracefully
    - That might mean adding some sort of metadata for the constraint or have a side-table that keeps track of that for them in a non-intrusive way
  - [x] Turn `ParseError`s into diagnostics
    - [ ] Have them look good
- [ ] More semantic analysis
  - [x] Check that all path return the right type
  - [ ] Improve the type-checker
    - [ ] Formalize the interface system
      - [ ] Actually check that implementations implement the interface
      - [x] Keep track of what types implement which interface at a program-level (implemented as a set of queries on concrete types)
    - [x] Keep track of implicit things the compiler should do
      - [x] auto-deref
      - [x] function/method call informations
  - [ ] Improve type-inference
    - [ ] Improve the constraint solver
      - [ ] Might want to factor it out as its own type
      - [ ] Speculative execution / make constraints aware of each other: This will allow things like `This type has fields a and b, Foo is the only struct having both fields so it is Foo` which isn't possible for the moment
- [ ] Parsing
  - [x] We have no turbofish for the moment, which can make some struct / enum literals impossible to express without type hints. Look into either adding turbofish or some other syntax to remove ambiguity
  - [x] Make sure turbofish is everywhere we want it to be
  - [ ] Make sure turbofish implementation is resilient
    - [ ] Wherever turbofish isn't implemented, we get a compiler warning
  - [] Maybe rewrite the parser at one point, it's a bit of a mess
    - [x] Did some refactoring, the state isn't final though, but better than it used to be
  - [ ] See if we support unicode in files
  - [ ] Improve match branch parsing (For the moment we force them to be `{ ... }`)
- [x] Create typed IR from HIR + type inference
- [x] Create mid-level IR from typed IR
  - [x] Run borrow-checker on it
    - [ ] Make said borrow checker smarter (partial move / mut borrow)
  - [x] Run path analysis (Do all path return, etc...)
  - [ ] Maybe some language-specific optimizations ?
    - [ ] Constant folding struct/tuple field accesses etc... Like `Foo {.bar: 1, .baz: 2}.baz` would just become `2`
    - [ ] Inlining also, so that constant-folding can work across function-boundaries maybe
- [ ] Codegen
  - [x] Figure out what backend to use: Inkwell (LLVM), cranelift, ...
    - [ ] Went for inkwell, look for other interesting ones
  - [x] Do we want our own SSA IR before backend or will the mid-level IR be enough
    - [x] Yep, we even got two (MIR -> LIR -> LLVM)
    - [ ] Finish verification of LIR (Or start it, maybe)
    - [ ] Make a pretty printer for LIR, maybe and add it as a display target
    - [ ] Look into ZSTs spilling into LLVM. Maybe it's not important, but doesn't hurt to look into it

Other:

- [ ] Type-safe way of having concrete types instead of using TypeRef / TypeId everywhere, especially for MIR, and to enforce the fact that all types have been substituted
- [x] Remove `PartialTypeRef` and `PartialTypeArg` as TypeRef seems to do what they do (`TypeRef::Unkwown`)
- [ ] Make sure performance of the typechecker is acceptable
- [ ] Have like a `diagnose_solver(unresolved: &[], errors: &[])` function to create good diagnostic from solver error output
- [ ] Make builtin types be an enum maybe, at least not runtime salsa interned structs as it is now

## Long-term

- [ ] Make it expr-based instead of stmt-based. The language is statement-based for the moment, more like C than Rust. This is because this is simpler to implement for the moment. I can see us moving to expressions once the architecture is a bit more mature.
- [x] Implement the lsp
- [ ] Write a solid Zed extension (and other IDEs but I use Zed)
  - [x] WIP (Highlighting isn't all there yet)

## Ideas

- [ ] Make use of salsa's serialization to have a compilation cache and look into incremental compilation.
