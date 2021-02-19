use dirs;
use pubgrub::range::Range;
use pubgrub::solver::{resolve, DependencyProvider};
use pubgrub::type_aliases::Map;
use pubgrub::version::SemanticVersion as SemVer;
use std::path::PathBuf;
use std::str::FromStr;
use std::{error::Error, process::exit};
use ureq;

use pubgrub_dependency_provider_elm::dependency_provider::{
    ElmPackageProviderOffline, ElmPackageProviderOnline, ProjectAdapter, VersionStrategy,
};
use pubgrub_dependency_provider_elm::pkg_version::PkgVersion;
use pubgrub_dependency_provider_elm::project_config::{Pkg, ProjectConfig};

const HELP: &str = r#"
solve_deps

Solve dependencies of an Elm project or published package.
By default, try in offline mode first
and switch to online mode if that fails.

USAGE:
    solve_deps [FLAGS...] [author/package@version]
    For example:
        solve_deps
        solve_deps --help
        solve_deps --offline
        solve_deps ianmackenzie/elm-3d-scene@1.0.1
        solve_deps --offline jxxcarlson/elm-tar@4.0.0
        solve_deps --online-newest w0rm/elm-physics@5.1.1
        solve_deps --online-oldest lucamug/style-framework@1.1.0

FLAGS:
    --help                 # Print this message and exit
    --offline              # No network request, use only installed packages
    --online-newest        # Use the newest compatible version
    --online-oldest        # Use the oldest compatible version
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_option = |s: &String| s.starts_with("--");
    let (options, pkg): (Vec<String>, Vec<String>) = args.into_iter().partition(is_option);

    // Check for the --help option
    if options.contains(&"--help".to_string()) {
        println!("{}", HELP);
        exit(0);
    }

    let offline = options.contains(&"--offline".to_string());
    let mut online_strat = None;
    if options.contains(&"--online-newest".to_string()) {
        online_strat = Some(VersionStrategy::Newest);
    } else if options.contains(&"--online-oldest".to_string()) {
        online_strat = Some(VersionStrategy::Oldest);
    }
    let pkg_version = pkg.get(0).map(|p| PkgVersion::from_str(p).unwrap());
    run(pkg_version, offline, online_strat);
}

fn run(pkg_version: Option<PkgVersion>, offline: bool, online_strat: Option<VersionStrategy>) {
    match (offline, online_strat) {
        (true, _) => {
            eprintln!("Solving offline");
            let deps_provider = ElmPackageProviderOffline::new(elm_home(), "0.19.1");
            solve_deps(&deps_provider, &pkg_version);
        }
        (false, None) => {
            eprintln!("Solving offline");
            let deps_provider = ElmPackageProviderOffline::new(elm_home(), "0.19.1");
            if !solve_deps(&deps_provider, &pkg_version) {
                eprintln!("Offline solving failed, switching to online");
                run(pkg_version, false, Some(VersionStrategy::Newest));
            }
        }
        (false, Some(strat)) => {
            eprintln!("Solving online with strategy {:?}", &strat);
            let deps_provider = ElmPackageProviderOnline::new(
                elm_home(),
                "0.19.1",
                "https://package.elm-lang.org",
                http_fetch,
                strat,
            )
            .expect("Error initializing the online dependency provider");
            solve_deps(&deps_provider, &pkg_version);
            // Save the versions cache
            deps_provider.save_cache().unwrap();
        }
    };
}

fn solve_deps<DP: DependencyProvider<Pkg, SemVer>>(
    deps_provider: &DP,
    pkg_version: &Option<PkgVersion>,
) -> bool {
    match pkg_version {
        // No package in CLI arguments so we solve deps of the elm project in the current directory
        None => {
            let version = SemVer::new(0, 0, 0);
            let elm_json_str = std::fs::read_to_string("elm.json")
                .expect("Are you in an elm project? there was an issue loading the elm.json");
            let project: ProjectConfig = serde_json::from_str(&elm_json_str).unwrap();
            match project {
                ProjectConfig::Application(app_config) => {
                    let pkg_id = Pkg::new("root", "");
                    let direct_deps: Map<Pkg, Range<SemVer>> = app_config
                        .dependencies
                        .direct
                        .into_iter()
                        .map(|(p, v)| (p, Range::exact(v)))
                        .collect();
                    let deps_provider = ProjectAdapter::new(
                        pkg_id.clone(),
                        version.clone(),
                        &direct_deps,
                        deps_provider,
                    );
                    resolve_helper(pkg_id, version, &deps_provider)
                }
                ProjectConfig::Package(pkg_config) => {
                    let pkg_id = pkg_config.name.clone();
                    let direct_deps: Map<Pkg, Range<SemVer>> = pkg_config
                        .dependencies
                        .into_iter()
                        .map(|(p, c)| (p, c.0))
                        .collect();
                    let deps_provider = ProjectAdapter::new(
                        pkg_id.clone(),
                        version.clone(),
                        &direct_deps,
                        deps_provider,
                    );
                    resolve_helper(pkg_id, version, &deps_provider)
                }
            }
        }
        // A published package was directly provided as CLI argument
        Some(pkg_v) => {
            let author = &pkg_v.author_pkg.author;
            let pkg = &pkg_v.author_pkg.pkg;
            let pkg_id = Pkg::new(author, pkg);
            resolve_helper(pkg_id, pkg_v.version, deps_provider)
        }
    }
}

fn resolve_helper<DP: DependencyProvider<Pkg, SemVer>>(
    pkg_id: Pkg,
    version: SemVer,
    deps_provider: &DP,
) -> bool {
    match resolve(deps_provider, pkg_id, version) {
        Ok(all_deps) => {
            let mut all_deps_formatted: Vec<_> = all_deps
                .iter()
                .map(|(p, v)| format!("{}@{}", p, v))
                .collect();
            all_deps_formatted.sort();
            eprintln!("{:#?}", all_deps_formatted);
            true
        }
        Err(err) => {
            eprintln!("{:?}", err);
            false
        }
    }
}

fn elm_home() -> PathBuf {
    match std::env::var_os("ELM_HOME") {
        None => default_elm_home(),
        Some(os_string) => os_string.into(),
    }
}

#[cfg(target_family = "unix")]
fn default_elm_home() -> PathBuf {
    dirs::home_dir()
        .expect("Unknown home directory")
        .join(".elm")
}

#[cfg(target_family = "windows")]
fn default_elm_home() -> PathBuf {
    dirs::data_dir()
        .expect("Unknown data directory")
        .join("elm")
}

fn http_fetch(url: &str) -> Result<String, Box<dyn Error>> {
    ureq::get(url)
        .timeout_connect(10_000)
        .call()
        .into_string()
        .map_err(|e| e.into())
}
