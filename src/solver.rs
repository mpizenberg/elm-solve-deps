use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::error::Error;
use std::path::PathBuf;

use pubgrub::error::PubGrubError;
use pubgrub::solver::DependencyProvider;
use pubgrub::type_aliases::Map;
use pubgrub::version::SemanticVersion as SemVer;
use pubgrub::{range::Range, solver::Dependencies};

use crate::dependency_provider::ProjectAdapter;
use crate::pkg_version::{Cache, PkgVersion};
use crate::project_config::{AppDependencies, PackageConfig, Pkg, PkgParseError, ProjectConfig};

pub fn solve_deps_with<Fetch, L, Versions>(
    project_elm_json: &str,
    fetch_elm_json: Fetch,
    list_available_versions: L,
) -> Result<AppDependencies, PubGrubError<Pkg, SemVer>>
where
    Fetch: Fn(&Pkg, SemVer) -> String,
    L: Fn(&Pkg) -> Versions,
    Versions: Iterator<Item = SemVer>,
{
    let solver = Solver {
        fetch_elm_json,
        list_available_versions,
    };
    let config: ProjectConfig = serde_json::from_str(project_elm_json).expect("Invalid elm.json");
    match config {
        ProjectConfig::Application(app_config) => {
            let normal_deps = app_config.dependencies.direct.iter();
            let direct_deps: Map<Pkg, Range<SemVer>> = normal_deps
                .chain(app_config.test_dependencies.direct.iter())
                .map(|(p, v)| (p.clone(), Range::exact(*v)))
                .collect();
            // TODO: take somehow into account already picked versions for indirect deps?
            solve_helper(&Pkg::new("root", ""), SemVer::zero(), direct_deps, solver)
        }
        ProjectConfig::Package(pkg_config) => {
            let normal_deps = pkg_config.dependencies.iter();
            let deps: Map<Pkg, Range<SemVer>> = normal_deps
                .chain(pkg_config.test_dependencies.iter())
                .map(|(p, c)| (p.clone(), c.0.clone()))
                .collect();
            solve_helper(&pkg_config.name, pkg_config.version, deps, solver)
        }
    }
}

/// Transform the generic solver into one that is specific to the current project
/// with the given root package version.
///
/// TODO: handle error case.
fn solve_helper<Fetch, L, Versions>(
    root_pkg: &Pkg,
    root_version: SemVer,
    direct_deps: Map<Pkg, Range<SemVer>>,
    solver: Solver<Fetch, L, Versions>,
) -> Result<AppDependencies, PubGrubError<Pkg, SemVer>>
where
    Fetch: Fn(&Pkg, SemVer) -> String,
    L: Fn(&Pkg) -> Versions,
    Versions: Iterator<Item = SemVer>,
{
    // Transform the generic dependency solver into one that is specific for the current project.
    let project_deps_provider =
        ProjectAdapter::new(root_pkg.clone(), root_version, &direct_deps, &solver);

    // Solve dependencies and remove the root dependency from the solution.
    let mut solution =
        pubgrub::solver::resolve(&project_deps_provider, root_pkg.clone(), root_version)?;
    solution.remove(root_pkg);

    // Split solution into direct and indirect deps.
    let (direct, indirect) = solution
        .into_iter()
        .partition(|(pkg, _)| direct_deps.contains_key(pkg));
    Ok(AppDependencies { direct, indirect })
}

#[derive(Debug, Clone)]
/// A type that implements the `DependencyProvider` trait
/// to be able to solve dependencies with pubgrub.
struct Solver<Fetch, L, Versions>
where
    Fetch: Fn(&Pkg, SemVer) -> String,
    L: Fn(&Pkg) -> Versions,
    Versions: Iterator<Item = SemVer>,
{
    fetch_elm_json: Fetch,
    list_available_versions: L,
}

impl<Fetch, L, Versions> DependencyProvider<Pkg, SemVer> for Solver<Fetch, L, Versions>
where
    Fetch: Fn(&Pkg, SemVer) -> String,
    L: Fn(&Pkg) -> Versions,
    Versions: Iterator<Item = SemVer>,
{
    /// Use `self.list_available_versions` and pick the package with the fewest versions.
    fn choose_package_version<T: Borrow<Pkg>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn Error>> {
        let potential_packages: Vec<_> = potential_packages.collect();
        Ok(pubgrub::solver::choose_package_with_fewest_versions(
            |p| (self.list_available_versions)(p.borrow()).into_iter(),
            potential_packages.into_iter(),
        ))
    }

    /// Load the dependencies from the elm.json retrieved with `self.fetch_elm_json`.
    fn get_dependencies(
        &self,
        package: &Pkg,
        version: &SemVer,
    ) -> Result<Dependencies<Pkg, SemVer>, Box<dyn Error>> {
        let pkg_config_str = (self.fetch_elm_json)(package, *version);
        let pkg_config: PackageConfig =
            serde_json::from_str(&pkg_config_str).expect("Invalid elm.json");
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
// OFFLINE #####################################################################
// #############################################################################

#[derive(Debug, Clone)]
pub struct Offline {
    elm_home: PathBuf,
    elm_version: String,
    versions_cache: RefCell<Cache>,
}

impl Offline {
    pub fn new<PB: Into<PathBuf>, S: ToString>(elm_home: PB, elm_version: S) -> Self {
        Offline {
            elm_home: elm_home.into(),
            elm_version: elm_version.to_string(),
            versions_cache: RefCell::new(Cache::new()),
        }
    }

    pub fn solve_deps<Fetch, L, Versions>(
        &self,
        project_elm_json: &str,
    ) -> Result<AppDependencies, PubGrubError<Pkg, SemVer>> {
        let fetch_elm_json = |pkg: &Pkg, ver| self.load_config(pkg, ver).unwrap();
        let list_available_versions =
            |pkg: &Pkg| self.load_installed_versions_of(pkg).unwrap().into_iter();
        solve_deps_with(project_elm_json, fetch_elm_json, list_available_versions)
    }

    /// Load existing versions already installed for the potential packages.
    ///
    /// Self is mutated to update the cache but we are cheating with RefCell
    /// to make it believe that it's not mutated.
    /// This is to be able to use the dependency provider,
    /// and I think it is OK as long as we don't make this function public?
    fn load_installed_versions_of(&self, pkg: &Pkg) -> Result<Vec<SemVer>, PkgParseError> {
        match self.versions_cache.borrow().cache.get(pkg) {
            Some(versions) => Ok(versions.iter().rev().cloned().collect()),
            None => {
                // Only load versions existing in elm home for packages we see for the first time.
                let versions: BTreeSet<SemVer> =
                    Cache::list_installed_versions(&self.elm_home, &self.elm_version, pkg)?;
                let sorted_versions = versions.iter().rev().cloned().collect();
                let cache = &mut self.versions_cache.borrow_mut().cache;
                cache.insert(pkg.clone(), versions);
                Ok(sorted_versions)
            }
        }
    }

    /// Load for ELM_HOME the `elm.json` config of a give package.
    fn load_config(&self, pkg: &Pkg, version: SemVer) -> Result<String, std::io::Error> {
        let pkg_version = PkgVersion {
            author_pkg: pkg.clone(),
            version,
        };
        let config_path = pkg_version.config_path(&self.elm_home, &self.elm_version);
        std::fs::read_to_string(&config_path)
    }
}

// #############################################################################
// ONLINE ######################################################################
// #############################################################################

#[derive(Debug, Clone)]
pub struct Online<F: Fn(&str) -> Result<String, Box<dyn Error>>> {
    offline: Offline,
    online_cache: Cache,
    remote: String,
    http_fetch: F,
    strategy: VersionStrategy,
}

#[derive(Debug, Clone)]
pub enum VersionStrategy {
    Newest,
    Oldest,
}

impl<F: Fn(&str) -> Result<String, Box<dyn Error>>> Online<F> {
    /// At the beginning we make one call to
    /// https://package.elm-lang.org/packages/since/...
    /// to update our list of existing packages.
    pub fn new<S: ToString>(
        offline: Offline,
        remote: S,
        http_fetch: F,
        strategy: VersionStrategy,
    ) -> Result<Self, Box<dyn Error>> {
        let mut online_cache = Cache::load(&offline.elm_home).unwrap_or_else(|_| Cache::new());
        let remote = remote.to_string();
        online_cache.update(&remote, &http_fetch)?;
        online_cache.save(&offline.elm_home)?;
        Ok(Self {
            offline,
            online_cache,
            remote,
            http_fetch,
            strategy,
        })
    }

    //     pub fn solve_deps<Fetch, L, Versions>(
    //         &self,
    //         project_elm_json: &str,
    //     ) -> Result<AppDependencies, PubGrubError<Pkg, SemVer>> {
    //         let fetch_elm_json = |pkg: &Pkg, ver| self.load_config(pkg, ver).unwrap();
    // //         let pkg_version = PkgVersion {
    // //             author_pkg: package.clone(),
    // //             version: *version,
    // //         };
    // //         let pkg_config = pkg_version
    // //             .load_config(&self.elm_home, &self.elm_version)
    // //             .or_else(|_| pkg_version.load_from_cache(&self.elm_home))
    // //             .or_else(|_| {
    // //                 pkg_version.fetch_config(&self.elm_home, &self.remote, &self.http_fetch)
    // //             })?;
    // //         let deps_iter = pkg_config
    // //             .dependencies_iter()
    // //             .map(|(p, r)| (p.clone(), r.clone()));
    // //         Ok(Dependencies::Known(deps_iter.collect()))
    //         let list_available_versions =
    //             |pkg: &Pkg| self.load_installed_versions_of(pkg).unwrap().into_iter();
    //         solve_deps_with(project_elm_json, fetch_elm_json, list_available_versions)
    //     }
}

// impl<F: Fn(&str) -> Result<String, Box<dyn Error>>> DependencyProvider<Pkg, SemVer>
//     for ElmPackageProviderOnline<F>
// {
//     /// For choose_package_version, we simply use the pubgrub helper function:
//     /// choose_package_with_fewest_versions
//     fn choose_package_version<T: Borrow<Pkg>, U: Borrow<Range<SemVer>>>(
//         &self,
//         potential_packages: impl Iterator<Item = (T, U)>,
//     ) -> Result<(T, Option<SemVer>), Box<dyn std::error::Error>> {
//         // Update the local cache of already downloaded packages.
//         let potential_packages: Vec<_> = potential_packages.collect();
//         self.offline_provider
//             .load_installed_versions_of(potential_packages.iter().map(|(p, _)| p.borrow()))?;
//         // Use the helper function from pubgrub to choose a package.
//         let empty_tree = BTreeSet::new();
//         let list_available_versions = |package: &Pkg| {
//             let local_cache = self.offline_provider.versions_cache.borrow();
//             let local_versions = local_cache.cache.get(package).unwrap_or(&empty_tree);
//             let online_cache = &self.online_versions_cache.cache;
//             let online_versions = online_cache.get(package).unwrap_or(&empty_tree);
//             // Combine local versions with online versions.
//             let all_versions: Vec<SemVer> =
//                 local_versions.union(online_versions).cloned().collect();
//             let iter: Box<dyn Iterator<Item = SemVer>> = match self.strategy {
//                 VersionStrategy::Oldest => Box::new(all_versions.into_iter()),
//                 VersionStrategy::Newest => Box::new(all_versions.into_iter().rev()),
//             };
//             iter
//         };
//
//         Ok(pubgrub::solver::choose_package_with_fewest_versions(
//             list_available_versions,
//             potential_packages.into_iter(),
//         ))
//     }
//
//     /// For get_dependencies, we check if those have been cached already,
//     /// otherwise we check if the package is installed on the disk and read there,
//     /// otherwise we ask for dependencies on the network.
//     fn get_dependencies(
//         &self,
//         package: &Pkg,
//         version: &SemVer,
//     ) -> Result<Dependencies<Pkg, SemVer>, Box<dyn Error>> {
//         let pkg_version = PkgVersion {
//             author_pkg: package.clone(),
//             version: *version,
//         };
//         let pkg_config = pkg_version
//             .load_config(&self.elm_home, &self.elm_version)
//             .or_else(|_| pkg_version.load_from_cache(&self.elm_home))
//             .or_else(|_| {
//                 pkg_version.fetch_config(&self.elm_home, &self.remote, &self.http_fetch)
//             })?;
//         let deps_iter = pkg_config
//             .dependencies_iter()
//             .map(|(p, r)| (p.clone(), r.clone()));
//         Ok(Dependencies::Known(deps_iter.collect()))
//     }
// }
