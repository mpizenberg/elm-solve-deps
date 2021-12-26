# Dependency solving for the elm ecosystem

This repository holds three utilities, each in its directory, with slightly different use cases, to deal with dependencies in the elm ecosystem.

 - `elm-solve-deps-lib/`: a Rust library, based on the [pubgrub crate][pubgrub] providing a set of types, functions and traits to deal with elm dependencies.
 - `elm-solve-deps-bin/`: a CLI executable providing a dedicated tool to handle elm dependencies.
 - `elm-solve-deps-wasm/`: a WebAssembly package, published on npm, to be able to use this solver directly in JavaScript.

[pubgrub]: https://github.com/pubgrub-rs/pubgrub
