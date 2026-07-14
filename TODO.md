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
      - [x] Keep track of what types implement which interface at a program-level (For the moment the work is done for every function-like thing)
    - [x] Keep track of implicit things the compiler should do
      - [x] auto-deref
      - [x] function/method call informations
  - [ ] Improve type-inference
    - [ ] Improve the constraint solver
      - [ ] Might want to factor it out as its own type
      - [ ] Speculative execution / make constraints aware of each other: This will allow things like `This type has fields a and b, Foo is the only struct having both fields so it is Foo` which isn't possible for the moment
- [ ] Parsing
  - [x] We have no turbofish for the moment, which can make some struct / enum literals impossible to express without type hints. Look into either adding turbofish or some other syntax to remove ambiguity
- [x] Create typed IR from HIR + type inference
- [x] Create mid-level IR from typed IR
  - [x] Run borrow-checker on it
    - [ ] Make said borrow checker smarter (partial move / mut borrow)
  - [x] Run path analysis (Do all path return, etc...)
  - [ ] Maybe some language-specific optimizations ?
- [ ] Codegen
  - [x] Figure out what backend to use: Inkwell (LLVM), cranelift, ...
    - [ ] Went for inkwell, look for other interesting ones
  - [x] Do we want our own SSA IR before backend or will the mid-level IR be enough
    - [ ] Yep, we even got two (MIR -> LIR -> LLVM)

## Long-term

- [ ] Make it expr-based instead of stmt-based. The language is statement-based for the moment, more like C than Rust. This is because this is simpler to implement for the moment. I can see us moving to expressions once the architecture is a bit more mature.
- [ ] Implement the lsp
- [ ] Write a solid Zed extension (and other IDEs but I use Zed)

## Ideas

- [ ] Make use of salsa's serialization to have a compilation cache and look into incremental compilation.
