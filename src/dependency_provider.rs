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

use crate::pkg_version::{Cache, PkgVersion};
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
    /// Load existing versions already installed for the potential packages.
    fn load_installed_versions_of<'a>(
        &self,
        packages: impl Iterator<Item = &'a Pkg>,
    ) -> Result<(), Box<dyn Error>> {
        for pkg in packages {
            // If we've already looked for versions of that package
            // just skip it and continue with the next one
            let cache = self.versions_cache.borrow();
            if cache.cache.contains_key(pkg) {
                continue;
            }
            drop(cache);
            let versions: BTreeSet<SemVer> =
                Cache::list_installed_versions(&self.elm_home, &self.elm_version, pkg)?;
            let mut cache = self.versions_cache.borrow_mut();
            cache.cache.insert(pkg.clone(), versions);
        }
        Ok(())
    }
}

impl DependencyProvider<Pkg, SemVer> for ElmPackageProviderOffline {
    /// We can only pick versions existing on the file system.
    /// In addition, we only want to query the file system once per package needed.
    /// So, the first time we want the list of versions for a given package,
    /// we walk the cache of installed versions in ~/.elm/0.19.1/packages/author/package/
    /// and register in an OfflineDependencyProvider those packages.
    /// Then we can call offlineProvider.choose_package_version(...).
    ///
    /// TODO: Improve by only reading the existing versions
    /// and only deserialize a version elm.json later in get_dependencies.
    fn choose_package_version<T: Borrow<Pkg>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn Error>> {
        let potential_packages: Vec<_> = potential_packages.collect();
        self.load_installed_versions_of(potential_packages.iter().map(|(p, _)| p.borrow()))?;
        // Use the helper function from pubgrub to choose a package.
        let empty_tree = BTreeSet::new();
        let list_available_versions = |package: &Pkg| {
            let cache = self.versions_cache.borrow();
            let versions = cache.cache.get(package).unwrap_or(&empty_tree).clone();
            versions.into_iter().rev()
        };
        Ok(pubgrub::solver::choose_package_with_fewest_versions(
            list_available_versions,
            potential_packages.into_iter(),
        ))
    }

    /// Load the dependencies from the elm.json of the installed package.
    fn get_dependencies(
        &self,
        package: &Pkg,
        version: &SemVer,
    ) -> Result<Dependencies<Pkg, SemVer>, Box<dyn Error>> {
        let pkg_version = PkgVersion {
            author_pkg: package.clone(),
            version: *version,
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
    online_versions_cache: Cache,
    offline_provider: ElmPackageProviderOffline,
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
        let elm_version = elm_version.to_string();
        let mut versions_cache = Cache::load(&elm_home).unwrap_or_else(|_| Cache::new());
        let remote = remote.to_string();
        versions_cache.update(&remote, &http_fetch)?;
        let offline_provider =
            ElmPackageProviderOffline::new(elm_home.clone(), elm_version.clone());
        Ok(ElmPackageProviderOnline {
            elm_home,
            elm_version,
            remote,
            online_versions_cache: versions_cache,
            offline_provider,
            http_fetch,
            strategy,
        })
    }

    /// Save the cache of existing versions.
    pub fn save_cache(&self) -> Result<(), crate::pkg_version::CacheError> {
        self.online_versions_cache.save(&self.elm_home)
    }
}

impl<F: Fn(&str) -> Result<String, Box<dyn Error>>> DependencyProvider<Pkg, SemVer>
    for ElmPackageProviderOnline<F>
{
    /// For choose_package_version, we simply use the pubgrub helper function:
    /// choose_package_with_fewest_versions
    fn choose_package_version<T: Borrow<Pkg>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn std::error::Error>> {
        // Update the local cache of already downloaded packages.
        let potential_packages: Vec<_> = potential_packages.collect();
        self.offline_provider
            .load_installed_versions_of(potential_packages.iter().map(|(p, _)| p.borrow()))?;
        // Use the helper function from pubgrub to choose a package.
        let empty_tree = BTreeSet::new();
        let list_available_versions = |package: &Pkg| {
            let local_cache = self.offline_provider.versions_cache.borrow();
            let local_versions = local_cache.cache.get(package).unwrap_or(&empty_tree);
            let online_versions = self
                .online_versions_cache
                .cache
                .get(package)
                .unwrap_or(&empty_tree);
            // Combine local versions with online versions.
            let all_versions: Vec<SemVer> =
                local_versions.union(online_versions).cloned().collect();
            let iter: Box<dyn Iterator<Item = SemVer>> = match self.strategy {
                VersionStrategy::Oldest => Box::new(all_versions.into_iter()),
                VersionStrategy::Newest => Box::new(all_versions.into_iter().rev()),
            };
            iter
        };

        Ok(pubgrub::solver::choose_package_with_fewest_versions(
            list_available_versions,
            potential_packages.into_iter(),
        ))
    }

    /// For get_dependencies, we check if those have been cached already,
    /// otherwise we check if the package is installed on the disk and read there,
    /// otherwise we ask for dependencies on the network.
    fn get_dependencies(
        &self,
        package: &Pkg,
        version: &SemVer,
    ) -> Result<Dependencies<Pkg, SemVer>, Box<dyn Error>> {
        let pkg_version = PkgVersion {
            author_pkg: package.clone(),
            version: *version,
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
