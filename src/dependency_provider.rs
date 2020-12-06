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
// - prioritized: no restriction, but installed packages are prioritized.
// - progressive (default): try offline first, if it fails switch to prioritized.
//
// ## offline
//
// We use the OfflineDependencyProvider as a base for this.
//
// For choose_package_version, we can only pick versions existing on the file system.
// In addition, we only want to query the file system once per package needed.
// So, the first time we want the list of versions for a given package,
// we walk the cache of installed versions in ~/.elm/0.19.1/packages/author/package/
// and register in an OfflineDependencyProvider those packages.
// Then we can call offlineProvider.choose_package_version(...).
//
// For get_dependencies, we can directly call offlineProvider.get_dependencies()
// since we have already registered packages with their dependencies
// when walking the cache of installed versions in choose_package_version.
//
// Rmq: this can be slightly more efficient if instead of OfflineDependencyProvider,
// we make our own, in which we store existing packages and dependencies
// in two different fields, to avoid the need of loading the elm.json of all versions
// when we just want the existing versions.
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
// ## prioritized
//
// At the beginning we update the list of existing packages just like in online mode.
//
// For choose_package_version, we can prioritize installed packages.
// A concrete way of doing it is using the choose_package_with_fewest_versions strategy
// with a function that list only installed versions.
// If that returns no package, we call it again with the full list of existing packages.
//
// For get_dependencies, we do the same that in online mode.
//
// ## progressive (default)
//
// Try to resolve dependencies in offline mode.
// If it fails, repeat in prioritized mode.

use pubgrub::range::Range;
use pubgrub::solver::{Dependencies, DependencyProvider, OfflineDependencyProvider};
use pubgrub::version::SemanticVersion as SemVer;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;

use crate::pkg_version::{Cache, Pkg, PkgVersion};

// #############################################################################
// OFFLINE #####################################################################
// #############################################################################

pub struct ElmPackageProviderOffline {
    elm_home: PathBuf,
    elm_version: String,
    cache: RefCell<OfflineDependencyProvider<String, SemVer>>,
}

impl ElmPackageProviderOffline {
    pub fn new<PB: Into<PathBuf>, S: ToString>(elm_home: PB, elm_version: S) -> Self {
        ElmPackageProviderOffline {
            elm_home: elm_home.into(),
            elm_version: elm_version.to_string(),
            cache: RefCell::new(OfflineDependencyProvider::new()),
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
    fn choose_package_version<T: Borrow<String>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn std::error::Error>> {
        let mut initial_potential_packages = Vec::new();
        for (pkg, range) in potential_packages {
            // If we've already looked for versions of that package
            // just skip it and continue with the next one
            if self.cache.borrow().versions(pkg.borrow()).is_some() {
                initial_potential_packages.push((pkg, range));
                continue;
            }

            let p = Pkg::from_str(pkg.borrow()).unwrap();
            let p_dir = p.config_path(&self.elm_home, &self.elm_version);
            let sub_dirs = match std::fs::read_dir(&p_dir) {
                Ok(s) => s,
                Err(_) => {
                    // The directory does not exist so probably
                    // no version of this package have ever been installed.
                    initial_potential_packages.push((pkg, range));
                    continue;
                }
            };

            // List installed versions
            let versions: Vec<SemVer> = sub_dirs
                .filter_map(|f| f.ok())
                // only keep directories
                .filter(|entry| entry.file_type().map(|f| f.is_dir()).unwrap_or(false))
                // retrieve the directory name as a string
                .filter_map(|entry| entry.file_name().into_string().ok())
                // convert into a version
                .filter_map(|s| SemVer::from_str(&s).ok())
                .collect();

            // Deserialize and register those versions into the cache
            for v in versions.into_iter() {
                let pkg_version = PkgVersion {
                    author_pkg: p.clone(),
                    version: v,
                };
                let pkg_config = pkg_version.load_config(&self.elm_home, &self.elm_version)?;
                let mut cache = self.cache.borrow_mut();
                cache.add_dependencies(
                    pkg_config.name.clone(),
                    pkg_config.version.clone(),
                    pkg_config
                        .dependencies_iter()
                        .map(|(p, r)| (p.clone(), r.clone())),
                );
            }
            initial_potential_packages.push((pkg, range));
        }

        // Let offline dependency provider choose the package version.
        self.cache
            .borrow()
            .choose_package_version(initial_potential_packages.into_iter())
    }

    fn get_dependencies(
        &self,
        package: &String,
        version: &SemVer,
    ) -> Result<Dependencies<String, SemVer>, Box<dyn std::error::Error>> {
        self.cache.borrow().get_dependencies(package, version)
    }
}

// #############################################################################
// ONLINE ######################################################################
// #############################################################################

pub struct ElmPackageProviderOnline<F: Fn(&str) -> Result<String, Box<dyn Error>>> {
    elm_home: PathBuf,
    elm_version: String,
    remote: String,
    pub versions_cache: Cache,
    http_fetch: F,
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
        })
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
            // List versions with latest first
            versions.iter().rev().cloned()
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
            .load_from_cache(&self.elm_home)
            .or_else(|_| pkg_version.load_config(&self.elm_home, &self.elm_version))
            .or_else(|_| {
                pkg_version.fetch_config(&self.elm_home, &self.remote, &self.http_fetch)
            })?;
        let deps_iter = pkg_config
            .dependencies_iter()
            .map(|(p, r)| (p.clone(), r.clone()));
        Ok(Dependencies::Known(deps_iter.collect()))
    }
}
