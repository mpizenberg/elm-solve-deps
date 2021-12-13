// SPDX-License-Identifier: MPL-2.0

//! # Dependency solving for the elm ecosystem
//!
//! The elm-solve-deps crate provides a set of types, functions and traits
//! to deal with dependencies in the elm ecosystem.
//! It is based on the [pubgrub crate][pubgrub] and powers the dependency solvers
//! of some of the tools in the elm ecosystem such as:
//!  - [elm-solve-deps]: a dedicated dependency solver executable
//!  - [elm-test-rs]: an alternative tests runner for elm
//!
//! [pubgrub]: https://github.com/pubgrub-rs/pubgrub
//! [elm-solve-deps]: https://github.com/mpizenberg/elm-solve-deps
//! [elm-test-rs]: https://github.com/mpizenberg/elm-test-rs
//!
//! The main objective of dependency solving is to start from
//! a set of dependency constraints, provided for example by the `elm.json` of a package:
//!
//! ```json
//! {
//!   ...,
//!   "dependencies": {
//!     "elm/core": "1.0.2 <= v < 2.0.0",
//!     "elm/http": "2.0.0 <= v < 3.0.0",
//!     "elm/json": "1.1.2 <= v < 2.0.0",
//!   },
//!   "test-dependencies": {
//!     "elm-explorations/test": "1.2.0 <= v < 2.0.0"
//!   }
//! }
//! ```
//!
//! And then find a set of package versions satisfying these constraints.
//! In general we also want some prioritization, such as picking the newest versions compatible.
//! In this case and at this date, without considering the test dependencies, the newest solution is:
//!
//! ```json
//! {
//!   "direct": {
//!     "elm/core": "1.0.5",
//!     "elm/http": "2.0.0",
//!     "elm/json": "1.1.3"
//!   },
//!   "indirect": {
//!     "elm/bytes": "1.0.8",
//!     "elm/file": "1.0.5",
//!     "elm/time": "1.0.0"
//!   }
//! }
//! ```
//!
//! And if we also consider the tests dependencies, we get instead:
//!
//! ```json
//! {
//!   "direct": {
//!     "elm/core": "1.0.5",
//!     "elm/http": "2.0.0",
//!     "elm/json": "1.1.3",
//!     "elm-explorations/test": "1.2.2"
//!   },
//!   "indirect": {
//!     "elm/bytes": "1.0.8",
//!     "elm/file": "1.0.5",
//!     "elm/html": "1.0.0",
//!     "elm/random": "1.0.0",
//!     "elm/time": "1.0.0",
//!     "elm/virtual-dom": "1.0.2"
//!   }
//! }
//! ```
//!
//! ## Simple offline dependency solver
//!
//! This library already provides a dependency solver ready for offline use cases.
//! The [`solver::Offline`] struct has to be initialized with the path to `ELM_HOME`,
//! as well as the version of elm used (concretely, this should only be `"0.19.1"` for now).
//! Then it provides a [`solve_deps`](solver::Offline::solve_deps) function,
//! which will either succeed and return a solution, or fail with an error.
//!
//! The offline solver will only ever look for packages inside `ELM_HOME` and thus
//! should work with other "elm-compatible" ecosystems such as Lamdera.
//! You can use it as follows.
//!
//! ```no_run
//! # use elm_solve_deps::solver;
//! # let elm_home = || "";
//! // Define an offline solver.
//! let offline_solver = solver::Offline::new(elm_home(), "0.19.1");
//!
//! // Load the project elm.json.
//! let elm_json_str = std::fs::read_to_string("elm.json")
//!     .expect("Are you in an elm project? there was an issue loading the elm.json");
//! let project_elm_json = serde_json::from_str(&elm_json_str)
//!     .expect("Failed to decode the elm.json");
//!
//! // Solve with tests dependencies.
//! let use_test = true;
//!
//! // Do not add any extra additional dependency.
//! let extras = &[];
//!
//! // Solve dependencies.
//! let solution = offline_solver
//!     .solve_deps(&project_elm_json, use_test, extras)
//!     .expect("Dependency solving failed");
//! ```
//!
//! Note that is is possible to provide additional package constraints,
//! which is convenient for tooling when requiring additional packages that are not recorded
//! directly in the original `elm.json` file.
//!
//! ## Online dependency solver
//!
//! We also provide an online solver for convenience.
//! When initialized, it starts by updating its database of known packages.
//! Then when solving dependencies, it works similarly than the offline server,
//! but with a set of packages that is the union of those existing locally,
//! and those existing on the package server.
//! Refer to [`solver::Online`] documentation for more info.
//!
//! ## Custom dependency solver
//!
//! Finally, if you want more control over the process of choosing dependencies,
//! you can either use the configurable function [`solver::solve_deps_with`],
//! or go with full customization by writing your own dependency provider
//! and use directly the pubgrub crate.
//!
//! When using [`solver::solve_deps_with`], you are required to provide
//! two functions, namely `fetch_elm_json` and `list_available_versions`,
//! implementing the following pseudo trait bounds:
//!
//! ```ignore
//! fetch_elm_json: Fn(&Pkg, SemVer) -> Result<PackageConfig, Error>
//! list_available_versions: Fn(&Pkg) -> Result<Iterator<SemVer>, Error>
//! ```
//!
//! It is up to you to figure out where to look for those config `elm.json`
//! and how to provide the list of existing versions.
//! Remark that the order in the versions iterator returned will correspond
//! to the prioritization for picking versions.
//! This means prioritizing newest or oldest versions is just a `.reverse()` on your part.
//!
//! ## Other helper modules
//!
//! In order for the different solver types to come together nicely,
//! a bunch of helper modules are also provided by this crate.
//!
//! - [`project_config`]: module dealing with the serialization and deserialization of config `elm.json` files.
//! - [`pkg_version`]: module defining the base type identifying a unique package version. It also
//! provides a few helper types and functions to read/write to a cache in `ELM_HOME` and to fetch
//! packages from a server following the same API than the official elm package server.
//! - [`constraint`]: module helping with serialization and deserialization of version constraints.
//! - [`dependency_provider`]: module with a helper implementation converting a generic dependency
//! provider into one that is using a project `elm.json` as root.

#![warn(missing_docs)]

pub mod constraint;
pub mod dependency_provider;
pub mod pkg_version;
pub mod project_config;
pub mod solver;
