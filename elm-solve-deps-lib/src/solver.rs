// SPDX-License-Identifier: MPL-2.0

//! Module providing helper functions to solve dependencies in the elm ecosystem.

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

use crate::constraint::Constraint;
use crate::dependency_provider::ProjectAdapter;
use crate::pkg_version::{Cache, CacheError, PkgVersion, PkgVersionError};
use crate::project_config::{AppDependencies, PackageConfig, Pkg, PkgParseError, ProjectConfig};

/// Advanced configurable function to solve dependencies of an elm project.
///
/// Set `use_test` to true to include test dependencies in the resolution.
///
/// Additional dependencies can be specified for convenience when they are not specified
/// directly in the project config, as follows.
///
/// ```
/// # use elm_solve_deps::project_config::Pkg;
/// # use elm_solve_deps::constraint::Constraint;
/// # use pubgrub::range::Range;
/// let extra = &[(
///   Pkg::new("jfmengels", "elm-review"),
///   Constraint(Range::between( (2,6,1), (3,0,0) )),
/// )];
/// ```
///
/// You are required to provide two functions,
/// namely `fetch_elm_json` and `list_available_versions`,
/// implementing the following pseudo trait bounds:
///
/// ```ignore
/// fetch_elm_json: Fn(&Pkg, SemVer) -> Result<PackageConfig, Error>
/// list_available_versions: Fn(&Pkg) -> Result<Iterator<SemVer>, Error>
/// ```
///
/// It is up to you to figure out where to look for those config `elm.json`
/// and how to provide the list of existing versions.
/// Remark that the order in the versions iterator returned will correspond
/// to the prioritization for picking versions.
/// This means prioritizing newest or oldest versions is just a `.reverse()` on your part.
pub fn solve_deps_with<Fetch, L, Versions>(
    project_elm_json: &ProjectConfig,
    use_test: bool,
    additional_constraints: &[(Pkg, Constraint)],
    fetch_elm_json: Fetch,
    list_available_versions: L,
) -> Result<AppDependencies, PubGrubError<Pkg, SemVer>>
where
    Fetch: Fn(&Pkg, SemVer) -> Result<PackageConfig, Box<dyn Error>>,
    L: Fn(&Pkg) -> Result<Versions, Box<dyn Error>>,
    Versions: Iterator<Item = SemVer>,
{
    let solver = Solver {
        fetch_elm_json,
        list_available_versions,
    };
    match project_elm_json {
        ProjectConfig::Application(app_config) => {
            let normal_deps = app_config.dependencies.direct.iter();
            let test_deps = app_config.test_dependencies.direct.iter();
            // Merge normal and test dependencies if solving with "use_test".
            let mut direct_deps: Map<Pkg, Range<SemVer>> = if use_test {
                normal_deps
                    .chain(test_deps)
                    .map(|(p, v)| (p.clone(), Range::exact(*v)))
                    .collect()
            } else {
                normal_deps
                    .map(|(p, v)| (p.clone(), Range::exact(*v)))
                    .collect()
            };
            // Include the additional constraints.
            for (p, r) in additional_constraints {
                let dep_range = direct_deps.entry(p.clone()).or_insert_with(Range::any);
                *dep_range = dep_range.intersection(&r.0);
            }
            // TODO: take somehow into account already picked versions for indirect deps?
            solve_helper(&Pkg::new("root", ""), SemVer::zero(), direct_deps, solver)
        }
        ProjectConfig::Package(pkg_config) => {
            let normal_deps = pkg_config.dependencies.iter();
            let test_deps = pkg_config.test_dependencies.iter();
            // Merge normal and test dependencies if solving with "use_test".
            let mut deps: Map<Pkg, Range<SemVer>> = if use_test {
                normal_deps
                    .chain(test_deps)
                    .map(|(p, c)| (p.clone(), c.0.clone()))
                    .collect()
            } else {
                normal_deps.map(|(p, c)| (p.clone(), c.0.clone())).collect()
            };
            // Include the additional constraints.
            for (p, r) in additional_constraints {
                let dep_range = deps.entry(p.clone()).or_insert_with(Range::any);
                *dep_range = dep_range.intersection(&r.0);
            }
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
    Fetch: Fn(&Pkg, SemVer) -> Result<PackageConfig, Box<dyn Error>>,
    L: Fn(&Pkg) -> Result<Versions, Box<dyn Error>>,
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
    Fetch: Fn(&Pkg, SemVer) -> Result<PackageConfig, Box<dyn Error>>,
    L: Fn(&Pkg) -> Result<Versions, Box<dyn Error>>,
    Versions: Iterator<Item = SemVer>,
{
    fetch_elm_json: Fetch,
    list_available_versions: L,
}

impl<Fetch, L, Versions> DependencyProvider<Pkg, SemVer> for Solver<Fetch, L, Versions>
where
    Fetch: Fn(&Pkg, SemVer) -> Result<PackageConfig, Box<dyn Error>>,
    L: Fn(&Pkg) -> Result<Versions, Box<dyn Error>>,
    Versions: Iterator<Item = SemVer>,
{
    /// Use `self.list_available_versions` and pick the package with the fewest versions.
    fn choose_package_version<T: Borrow<Pkg>, U: Borrow<Range<SemVer>>>(
        &self,
        potential_packages: impl Iterator<Item = (T, U)>,
    ) -> Result<(T, Option<SemVer>), Box<dyn Error>> {
        // TODO: replace by a versions of this that could fail when listing available packages.
        Ok(pubgrub::solver::choose_package_with_fewest_versions(
            |p| (self.list_available_versions)(p.borrow()).unwrap(),
            potential_packages,
        ))
    }

    /// Load the dependencies from the elm.json retrieved with `self.fetch_elm_json`.
    fn get_dependencies(
        &self,
        package: &Pkg,
        version: &SemVer,
    ) -> Result<Dependencies<Pkg, SemVer>, Box<dyn Error>> {
        // TODO: handle the unknown case (change fetch_elm_json signature)
        let pkg_config = (self.fetch_elm_json)(package, *version)?;
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

/// Dependency solver ready for offline use cases.
///
/// The [`Offline`] struct has to be initialized with the path to `ELM_HOME`,
/// as well as the version of elm used (concretely, this should only be `"0.19.1"` for now).
/// Then it provides a [`solve_deps`](Offline::solve_deps) function,
/// which will either succeed and return a solution, or fail with an error.
///
/// The offline solver will only ever look for packages inside `ELM_HOME` and thus
/// should work with other "elm-compatible" ecosystems such as Lamdera.
/// You can use it as follows.
///
/// ```no_run
/// # use elm_solve_deps::solver;
/// # let elm_home = || "";
/// // Define an offline solver.
/// let offline_solver = solver::Offline::new(elm_home(), "0.19.1");
///
/// // Load the project elm.json.
/// let elm_json_str = std::fs::read_to_string("elm.json")
///     .expect("Are you in an elm project? there was an issue loading the elm.json");
/// let project_elm_json = serde_json::from_str(&elm_json_str)
///     .expect("Failed to decode the elm.json");
///
/// // Solve with tests dependencies.
/// let use_test = true;
///
/// // Do not add any extra additional dependency.
/// let extras = &[];
///
/// // Solve dependencies.
/// let solution = offline_solver
///     .solve_deps(&project_elm_json, use_test, extras)
///     .expect("Dependency solving failed");
/// ```
///
/// Note that it is possible to provide additional package constraints,
/// which is convenient for tooling when requiring additional packages that are not recorded
/// directly in the original `elm.json` file.
#[derive(Debug, Clone)]
pub struct Offline {
    elm_home: PathBuf,
    elm_version: String,
    versions_cache: RefCell<Cache>,
}

impl Offline {
    /// Constructor for the offline solver.
    ///
    /// The `elm_home` argument will typically be `/home/user/.elm`.
    /// The `elm_version` argument should be "0.19.1"
    /// as it is currently the only version supported.
    pub fn new<PB: Into<PathBuf>, S: ToString>(elm_home: PB, elm_version: S) -> Self {
        Offline {
            elm_home: elm_home.into(),
            elm_version: elm_version.to_string(),
            versions_cache: RefCell::new(Cache::new()),
        }
    }

    /// Run the dependency solver on a given project config, obtained from an `elm.json`.
    ///
    /// Set `use_test` to `false` to solve the normal dependencies
    /// or to `true` to also take into account the test dependencies.
    ///
    /// Additional dependencies can be specified for convenience when they are not specified
    /// directly in the project config, as follows.
    ///
    /// ```
    /// # use elm_solve_deps::project_config::Pkg;
    /// # use elm_solve_deps::constraint::Constraint;
    /// # use pubgrub::range::Range;
    /// let extra = &[(
    ///   Pkg::new("jfmengels", "elm-review"),
    ///   Constraint(Range::between( (2,6,1), (3,0,0) )),
    /// )];
    /// ```
    pub fn solve_deps(
        &self,
        project_elm_json: &ProjectConfig,
        use_test: bool,
        additional_constraints: &[(Pkg, Constraint)],
    ) -> Result<AppDependencies, PubGrubError<Pkg, SemVer>> {
        let list_available_versions = |pkg: &Pkg| {
            self.load_installed_versions_of(pkg)
                .map(|vs| vs.into_iter())
                .map_err(|err| err.into())
        };
        let fetch_elm_json = |pkg: &Pkg, version| {
            let pkg_version = PkgVersion {
                author_pkg: pkg.clone(),
                version,
            };
            pkg_version
                .load_config(&self.elm_home, &self.elm_version)
                .map_err(|err| err.into())
        };
        solve_deps_with(
            project_elm_json,
            use_test,
            additional_constraints,
            fetch_elm_json,
            list_available_versions,
        )
    }

    /// Load existing versions already installed for the potential packages.
    ///
    /// Self is mutated to update the cache but we are cheating with RefCell
    /// to make it believe that it's not mutated.
    /// This is to be able to use the dependency provider,
    /// and I think it is OK as long as we don't make this function public?
    fn load_installed_versions_of(&self, pkg: &Pkg) -> Result<Vec<SemVer>, PkgParseError> {
        let versions_cache = self.versions_cache.borrow();
        match versions_cache.cache.get(pkg) {
            Some(versions) => Ok(versions.iter().rev().cloned().collect()),
            None => {
                drop(versions_cache);
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
}

// #############################################################################
// ONLINE ######################################################################
// #############################################################################

/// Online variant of the dependency solver.
///
/// When initialized, it starts by updating its database of known packages.
/// Then when solving dependencies, it works similarly than the [`Offline`] solver,
/// but with a set of packages that is the union of those existing locally,
/// and those existing on the package server.
#[derive(Debug, Clone)]
pub struct Online<F: Fn(&str) -> Result<String, Box<dyn Error + Send + Sync>>> {
    offline: Offline,
    online_cache: Cache,
    remote: String,
    http_fetch: F,
    strategy: VersionStrategy,
}

/// Strategy of an online solver, consisting of picking either the newest
/// or oldest compatible versions.
#[derive(Debug, Clone, Copy)]
pub enum VersionStrategy {
    /// Choose the newest compatible versions.
    Newest,
    /// Choose the oldest compatible versions.
    Oldest,
}

impl<F: Fn(&str) -> Result<String, Box<dyn Error + Send + Sync>>> Online<F> {
    /// Constructor for the online solver.
    ///
    /// At the beginning we make one call to
    /// `https://package.elm-lang.org/packages/since/...`
    /// to update our list of existing packages.
    ///
    /// The address of the remote package server is configurable
    /// in case you want to use a mirror of the package server.
    /// Typically, this should be set to `"https://package.elm-lang.org"`.
    ///
    /// The caller must also provide the http client to make the get requests.
    /// One simple option is to use the [`ureq`](https://crates.io/crates/ureq) crate for this.
    pub fn new<S: ToString>(
        offline: Offline,
        remote: S,
        http_fetch: F,
        strategy: VersionStrategy,
    ) -> Result<Self, CacheError> {
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

    /// Run the dependency solver on a given project config, obtained from an `elm.json`.
    ///
    /// See [`Offline::solve_deps`].
    pub fn solve_deps(
        &self,
        project_elm_json: &ProjectConfig,
        use_test: bool,
        additional_constraints: &[(Pkg, Constraint)],
    ) -> Result<AppDependencies, PubGrubError<Pkg, SemVer>> {
        let list_available_versions = |pkg: &Pkg| Ok(self.list_available_versions(pkg));
        let fetch_elm_json =
            |pkg: &Pkg, version| self.fetch_elm_json(pkg, version).map_err(|err| err.into());
        solve_deps_with(
            project_elm_json,
            use_test,
            additional_constraints,
            fetch_elm_json,
            list_available_versions,
        )
    }

    /// Try successively to load the elm.json of this package from
    ///  - the elm home,
    ///  - the online cache,
    ///  - or directly from the package website.
    fn fetch_elm_json(&self, pkg: &Pkg, version: SemVer) -> Result<PackageConfig, PkgVersionError> {
        let pkg_version = PkgVersion {
            author_pkg: pkg.clone(),
            version,
        };
        pkg_version
            .load_config(&self.offline.elm_home, &self.offline.elm_version)
            .or_else(|_| pkg_version.load_from_cache(&self.offline.elm_home))
            .or_else(|_| {
                pkg_version.fetch_config(&self.offline.elm_home, &self.remote, &self.http_fetch)
            })
    }

    /// Combine local versions with online versions listed on the package server.
    fn list_available_versions(&self, pkg: &Pkg) -> impl Iterator<Item = SemVer> {
        let empty_tree = BTreeSet::new();
        let local_cache = self.offline.versions_cache.borrow();
        let local_versions = local_cache.cache.get(pkg).unwrap_or(&empty_tree);
        let online_cache = &self.online_cache.cache;
        let online_versions = online_cache.get(pkg).unwrap_or(&empty_tree);
        let all_versions: Vec<SemVer> = local_versions.union(online_versions).cloned().collect();
        let iter: Box<dyn Iterator<Item = SemVer>> = match self.strategy {
            VersionStrategy::Oldest => Box::new(all_versions.into_iter()),
            VersionStrategy::Newest => Box::new(all_versions.into_iter().rev()),
        };
        iter
    }
}
