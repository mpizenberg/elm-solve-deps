use dirs;
use pubgrub::solver::{resolve, DependencyProvider};
use pubgrub::version::SemanticVersion as SemVer;
use std::path::PathBuf;
use std::str::FromStr;
use std::{error::Error, process::exit};
use ureq;

use pubgrub_dependency_provider_elm::dependency_provider::{
    ElmPackageProviderOffline, ElmPackageProviderOnline, VersionStrategy,
};
use pubgrub_dependency_provider_elm::pkg_version::PkgVersion;

const HELP: &str = r#"
solve_pkg_deps

Solve dependencies of an Elm package.
By default, try in offline mode first
and switch to online mode if that fails.

USAGE:
    solve_pkg_deps [FLAGS] author/package@version
    For example:
        solve_pkg_deps ianmackenzie/elm-3d-scene@1.0.1
        solve_pkg_deps --offline jxxcarlson/elm-tar@4.0.0
        solve_pkg_deps --online-newest w0rm/elm-physics@5.1.1
        solve_pkg_deps --online-oldest lucamug/style-framework@1.1.0

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
    let pkg_version = PkgVersion::from_str(&pkg[0]).unwrap();
    run(pkg_version, offline, online_strat);
}

fn run(pkg_version: PkgVersion, offline: bool, online_strat: Option<VersionStrategy>) {
    let author = &pkg_version.author_pkg.author;
    let pkg = &pkg_version.author_pkg.pkg;
    let author_pkg = format!("{}/{}", author, pkg);
    let version = pkg_version.version.clone();
    match (offline, online_strat) {
        (true, _) => {
            eprintln!("Solving offline");
            let deps_provider = ElmPackageProviderOffline::new(elm_home(), "0.19.1");
            resolve_deps(&deps_provider, author_pkg, version);
        }
        (false, None) => {
            eprintln!("Solving offline");
            let deps_provider = ElmPackageProviderOffline::new(elm_home(), "0.19.1");
            if !resolve_deps(&deps_provider, author_pkg, version) {
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
            resolve_deps(&deps_provider, author_pkg, version);
            // Save the versions cache
            deps_provider.save_cache().unwrap();
        }
    };
}

fn resolve_deps<DP: DependencyProvider<String, SemVer>>(
    deps_provider: &DP,
    pkg: String,
    version: SemVer,
) -> bool {
    match resolve(deps_provider, pkg, version) {
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
