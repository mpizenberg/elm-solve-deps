// SPDX-License-Identifier: MPL-2.0

//! Module with a helper implementation converting a generic dependency
//! provider into one that is using a project `elm.json` as root.

use pubgrub::range::Range;
use pubgrub::solver::{Dependencies, DependencyProvider};
use pubgrub::type_aliases::Map;
use pubgrub::version::SemanticVersion as SemVer;
use std::borrow::Borrow;
use std::error::Error;

use crate::project_config::Pkg;

/// Dependency provider of a package or an application elm project.
/// Will only work properly if used to resolve dependencies for its root.
///
/// ```ignore
/// let pkg_id: String = ...;
/// let version: SemVer = ...;
/// let project_dp = ProjectAdapter::new(pkg_id.clone(), version.clone(), ...);
/// let solution = resolve(&project_dp, pkg_id, version)?;
/// ```
pub struct ProjectAdapter<'a, DP: DependencyProvider<Pkg, SemVer>> {
    pkg_id: Pkg,
    version: SemVer,
    direct_deps: &'a Map<Pkg, Range<SemVer>>,
    deps_provider: &'a DP,
}

impl<'a, DP: DependencyProvider<Pkg, SemVer>> ProjectAdapter<'a, DP> {
    /// Initialize a project dependency provider.
    pub fn new(
        pkg_id: Pkg,
        version: SemVer,
        direct_deps: &'a Map<Pkg, Range<SemVer>>,
        deps_provider: &'a DP,
    ) -> Self {
        if pkg_id == Pkg::new("elm", "") {
            panic!(r#"Using "elm" for the root package id is forbidden"#)
        }
        Self {
            pkg_id,
            version,
            direct_deps,
            deps_provider,
        }
    }
}

impl<'a, DP: DependencyProvider<Pkg, SemVer>> DependencyProvider<Pkg, SemVer>
    for ProjectAdapter<'a, DP>
{
    /// The list of potential packages can never be empty,
    /// and the root package can only be asked alone, first.
    fn choose_package_version<T: Borrow<Pkg>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn Error>> {
        let mut potential_packages = potential_packages;
        let (p, r) = potential_packages.next().unwrap(); // unwrap ok since potential_packages must contains at least one item
        if p.borrow() == &self.pkg_id {
            return Ok((p, Some(self.version)));
        }
        self.deps_provider
            .choose_package_version(std::iter::once((p, r)).chain(potential_packages))
    }

    /// If asking for dependencies of the root package,
    /// overwrite with the project direct dependencies.
    fn get_dependencies(
        &self,
        package: &Pkg,
        version: &SemVer,
    ) -> Result<Dependencies<Pkg, SemVer>, Box<dyn Error>> {
        if package == &self.pkg_id {
            Ok(Dependencies::Known(self.direct_deps.clone()))
        } else {
            self.deps_provider.get_dependencies(package, version)
        }
    }
}
