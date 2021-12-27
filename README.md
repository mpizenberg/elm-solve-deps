# Dependency solving for the elm ecosystem

This repository holds two utilities, each in its directory, with slightly different use cases, to deal with dependencies in the elm ecosystem.

 - `elm-solve-deps-lib/`: a Rust library, based on the [pubgrub crate][pubgrub] providing a set of types, functions and traits to deal with elm dependencies.
 - `elm-solve-deps-bin/`: a CLI executable providing a dedicated tool to handle elm dependencies.

Another utility called `elm-solve-deps-wasm/`, is a WebAssembly package, published on npm, to be able to use this solver directly in JavaScript.
That one lives in another repository [due to issues][wasm-issues] compiling the wasm crate with different build profiles when using a Cargo workspace.

[pubgrub]: https://github.com/pubgrub-rs/pubgrub
[wasm-issues]: https://users.rust-lang.org/t/how-to-shrink-wasm-size-in-a-cargo-workspace/69399
