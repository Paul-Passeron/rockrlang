# TODO for Rockr

This can be a great starting point for contributors looking for something to do.

## Short-term
- [ ] Better diagnostics
  - [ ] Display diagnostics
  - [ ] Handle constraint solver output gracefully
    - That might mean adding some sort of metadata for the constraint or have a side-table that keeps track of that for them in a non-intrusive way
  - [ ] Turn `ParseError`s into diagnostics
- [ ] More semantic analysis
  - [ ] Check that all path return the right type
  - [ ] Improve the type-checker
    - [ ] Formalize the interface system
      - [ ] Actually check that implementations implement the interface
      - [ ] Keep track of what types implement which interface at a program-level (For the moment the work is done for every function-like thing)
    - [ ] Keep track of implicit things the compiler should do
      - [ ] auto-deref
      - [ ] function/method call informations
  - [ ] Improve type-inference
    - [ ] Improve the constraint solver
      - [ ] Might want to factor it out as its own type
      - [ ] Speculative execution / make constraints aware of each other: This will allow things like `This type has fields a and b, Foo is the only struct having both fields so it is Foo` which isn't possible for the moment
- [ ] Parsing
  - [ ] We have no turbofish for the moment, which can make some struct / enum literals impossible to express without type hints. Look into either adding turbofish or some other syntax to remove ambiguity

## Long-term
- [ ] Make it expr-based instead of stmt-based. The language is statement-based for the moment, more like C than Rust. This is because this is simpler to implement for the moment. I can see us moving to expressions once the architecture is a bit more mature.


## Ideas
- [ ] Make use of salsa's serialization to have a compilation cache and look into incremental compilation.
