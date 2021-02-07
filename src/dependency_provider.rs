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
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;

use crate::pkg_version::{Cache, Pkg, PkgVersion};

/// Dependency provider of a package or an application elm project.
/// Will only work properly if used to resolve dependencies for its root.
///
/// ```ignore
/// let pkg_id: String = ...;
/// let version: SemVer = ...;
/// let project_dp = ProjectAdapter::new(pkg_id.clone(), version.clone(), ...);
/// let solution = resolve(&project_dp, pkg_id, version)?;
/// ```
pub struct ProjectAdapter<'a, DP: DependencyProvider<String, SemVer>> {
    pkg_id: String,
    version: SemVer,
    direct_deps: &'a Map<String, Range<SemVer>>,
    deps_provider: &'a DP,
}

impl<'a, DP: DependencyProvider<String, SemVer>> ProjectAdapter<'a, DP> {
    /// Initialize a project dependency provider.
    pub fn new(
        pkg_id: String,
        version: SemVer,
        direct_deps: &'a Map<String, Range<SemVer>>,
        deps_provider: &'a DP,
    ) -> Self {
        if pkg_id.as_str() == "elm" {
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

impl<'a, DP: DependencyProvider<String, SemVer>> DependencyProvider<String, SemVer>
    for ProjectAdapter<'a, DP>
{
    /// The list of potential packages can never be empty,
    /// and the root package can only be asked alone, first.
    fn choose_package_version<T: Borrow<String>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn std::error::Error>> {
        let mut potential_packages = potential_packages;
        let (p, r) = potential_packages.next().unwrap();
        if p.borrow() == &self.pkg_id {
            return Ok((p, Some(self.version)));
        }
        self.deps_provider
            .choose_package_version(std::iter::once((p, r)).chain(potential_packages))
    }

    fn get_dependencies(
        &self,
        package: &String,
        version: &SemVer,
    ) -> Result<Dependencies<String, SemVer>, Box<dyn std::error::Error>> {
        if package == &self.pkg_id {
            Ok(Dependencies::Known(self.direct_deps.clone()))
        } else {
            self.deps_provider.get_dependencies(package, version)
        }
    }
}

// #############################################################################
// OFFLINE #####################################################################
// #############################################################################

#[derive(Debug, Clone)]
pub struct ElmPackageProviderOffline {
    elm_home: PathBuf,
    elm_version: String,
    versions_cache: RefCell<Cache>,
}

impl ElmPackageProviderOffline {
    pub fn new<PB: Into<PathBuf>, S: ToString>(elm_home: PB, elm_version: S) -> Self {
        ElmPackageProviderOffline {
            elm_home: elm_home.into(),
            elm_version: elm_version.to_string(),
            versions_cache: RefCell::new(Cache::new()),
        }
    }
}

impl DependencyProvider<String, SemVer> for ElmPackageProviderOffline {
    /// We can only pick versions existing on the file system.
    /// In addition, we only want to query the file system once per package needed.
    /// So, the first time we want the list of versions for a given package,
    /// we walk the cache of installed versions in ~/.elm/0.19.1/packages/author/package/
    /// and register in an OfflineDependencyProvider those packages.
    /// Then we can call offlineProvider.choose_package_version(...).
    ///
    /// TODO: Improve by only reading the existing versions
    /// and only deserialize a version elm.json later in get_dependencies.
    fn choose_package_version<T: Borrow<String>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn std::error::Error>> {
        let mut initial_potential_packages = Vec::new();
        for (pkg, range) in potential_packages {
            // If we've already looked for versions of that package
            // just skip it and continue with the next one
            let cache = self.versions_cache.borrow();
            if cache.cache.get(pkg.borrow()).is_some() {
                initial_potential_packages.push((pkg, range));
                continue;
            }
            drop(cache);
            let versions: BTreeSet<SemVer> =
                Cache::list_installed_versions(&self.elm_home, &self.elm_version, &pkg.borrow())?;
            let mut cache = self.versions_cache.borrow_mut();
            cache.cache.insert(pkg.borrow().clone(), versions);
            initial_potential_packages.push((pkg, range));
        }
        // Use the helper function from pubgrub to choose a package.
        let empty_tree = BTreeSet::new();
        let list_available_versions = |package: &String| {
            let cache = self.versions_cache.borrow();
            let versions = cache
                .cache
                .get(package)
                .unwrap_or_else(|| &empty_tree)
                .clone();
            versions.into_iter().rev()
        };
        Ok(pubgrub::solver::choose_package_with_fewest_versions(
            list_available_versions,
            initial_potential_packages.into_iter(),
        ))
    }

    /// Load the dependencies from the elm.json of the installed package.
    fn get_dependencies(
        &self,
        package: &String,
        version: &SemVer,
    ) -> Result<Dependencies<String, SemVer>, Box<dyn std::error::Error>> {
        let author_pkg = Pkg::from_str(package).unwrap();
        let pkg_version = PkgVersion {
            author_pkg,
            version: version.clone(),
        };
        let pkg_config = pkg_version.load_config(&self.elm_home, &self.elm_version)?;
        Ok(Dependencies::Known(
            pkg_config
                .dependencies
                .into_iter()
                .map(|(p, c)| (p, c.0))
                .collect(),
        ))
    }
}

// #############################################################################
// ONLINE ######################################################################
// #############################################################################

#[derive(Debug, Clone)]
pub struct ElmPackageProviderOnline<F: Fn(&str) -> Result<String, Box<dyn Error>>> {
    elm_home: PathBuf,
    elm_version: String,
    remote: String,
    versions_cache: Cache,
    http_fetch: F,
    strategy: VersionStrategy,
}

#[derive(Debug, Clone)]
pub enum VersionStrategy {
    Newest,
    Oldest,
}

impl<F: Fn(&str) -> Result<String, Box<dyn Error>>> ElmPackageProviderOnline<F> {
    /// At the beginning we make one call to
    /// https://package.elm-lang.org/packages/since/...
    /// to update our list of existing packages.
    pub fn new<PB: Into<PathBuf>, S: ToString>(
        elm_home: PB,
        elm_version: S,
        remote: S,
        http_fetch: F,
        strategy: VersionStrategy,
    ) -> Result<Self, Box<dyn Error>> {
        let elm_home = elm_home.into();
        let mut versions_cache = Cache::load(&elm_home).unwrap_or_else(|_| Cache::new());
        let remote = remote.to_string();
        versions_cache.update(&remote, &http_fetch)?;
        Ok(ElmPackageProviderOnline {
            elm_home,
            elm_version: elm_version.to_string(),
            remote,
            versions_cache,
            http_fetch,
            strategy,
        })
    }

    /// Save the cache of existing versions.
    pub fn save_cache(&self) -> Result<(), crate::pkg_version::CacheError> {
        self.versions_cache.save(&self.elm_home)
    }
}

impl<F: Fn(&str) -> Result<String, Box<dyn Error>>> DependencyProvider<String, SemVer>
    for ElmPackageProviderOnline<F>
{
    /// For choose_package_version, we simply use the pubgrub helper function:
    /// choose_package_with_fewest_versions
    fn choose_package_version<T: Borrow<String>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn std::error::Error>> {
        let empty_tree = BTreeSet::new();
        let list_available_versions = |package: &String| {
            let versions = self
                .versions_cache
                .cache
                .get(package)
                .unwrap_or_else(|| &empty_tree);
            let iter: Box<dyn Iterator<Item = SemVer>> = match self.strategy {
                VersionStrategy::Oldest => Box::new(versions.iter().cloned()),
                VersionStrategy::Newest => Box::new(versions.iter().rev().cloned()),
            };
            iter
        };
        Ok(pubgrub::solver::choose_package_with_fewest_versions(
            list_available_versions,
            potential_packages,
        ))
    }

    /// For get_dependencies, we check if those have been cached already,
    /// otherwise we check if the package is installed on the disk and read there,
    /// otherwise we ask for dependencies on the network.
    fn get_dependencies(
        &self,
        package: &String,
        version: &SemVer,
    ) -> Result<Dependencies<String, SemVer>, Box<dyn std::error::Error>> {
        let author_pkg = Pkg::from_str(&package).unwrap();
        let pkg_version = PkgVersion {
            author_pkg,
            version: version.clone(),
        };
        let pkg_config = pkg_version
            .load_config(&self.elm_home, &self.elm_version)
            .or_else(|_| pkg_version.load_from_cache(&self.elm_home))
            .or_else(|_| {
                pkg_version.fetch_config(&self.elm_home, &self.remote, &self.http_fetch)
            })?;
        let deps_iter = pkg_config
            .dependencies_iter()
            .map(|(p, r)| (p.clone(), r.clone()));
        Ok(Dependencies::Known(deps_iter.collect()))
    }
}
