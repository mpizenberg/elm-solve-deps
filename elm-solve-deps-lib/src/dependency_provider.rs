// # Dependency provider for Elm packages
//
// There are two methods to implement for a dependency provider:
//   1. choose_package_version
//   2. get_dependencies
//
// Those are the only part of the solver potentially doing IO.
// We want to minimize the amount of network calls and file system read.
//
// ## Connectivity modes
//
// - offline: use exclusively installed packages.
// - online: no network restriction to select packages.
// - progressive (default): try offline first, if it fails switch to prioritized.
//
// ## offline
//
// For choose_package_version, we can only pick versions existing on the file system.
// In addition, we only want to query the file system once per package needed.
// So, the first time we want the list of versions for a given package,
// we walk the cache of installed versions in ~/.elm/0.19.1/packages/author/package/
//
// For get_dependencies, we load the elm.json config of the installed package
// and extract dependencies from it.
//
// ## online
//
// At the beginning we make one call to https://package.elm-lang.org/packages/since/...
// to update our list of existing packages.
//
// For choose_package_version, we simply use the pubgrub helper function:
// choose_package_with_fewest_versions.
//
// For get_dependencies, we check if those have been cached already,
// otherwise we check if the package is installed on the disk and read there,
// otherwise we ask for dependencies on the network.
//
// ## progressive (default)
//
// Try to resolve dependencies in offline mode.
// If it fails, repeat in prioritized mode.

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
