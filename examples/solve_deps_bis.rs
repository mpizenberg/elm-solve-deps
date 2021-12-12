use dirs;
use std::path::PathBuf;
use std::str::FromStr;
use std::{error::Error, process::exit};
use ureq;

use pubgrub_dependency_provider_elm::pkg_version::PkgVersion;
use pubgrub_dependency_provider_elm::project_config::{AppDependencies, ProjectConfig};
use pubgrub_dependency_provider_elm::solver::{self, VersionStrategy};

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
    let maybe_pkg_version = pkg.get(0).map(|p| PkgVersion::from_str(p).unwrap());
    run(maybe_pkg_version, offline, online_strat);
}

fn run(
    maybe_pkg_version: Option<PkgVersion>,
    offline: bool,
    online_strat: Option<VersionStrategy>,
) {
    let elm_version = "0.19.1";

    // Load the elm.json of the package given as argument or of the current folder.
    let project_elm_json: ProjectConfig = match maybe_pkg_version.clone() {
        Some(pkg_version) => {
            let pkg_config = pkg_version
                .load_config(elm_home(), elm_version)
                .or_else(|_| pkg_version.load_from_cache(elm_home()))
                .or_else(|_| {
                    pkg_version.fetch_config(elm_home(), "https://package.elm-lang.org", http_fetch)
                })
                .unwrap();
            ProjectConfig::Package(pkg_config)
        }
        None => {
            let elm_json_str = std::fs::read_to_string("elm.json")
                .expect("Are you in an elm project? there was an issue loading the elm.json");
            serde_json::from_str(&elm_json_str).unwrap()
        }
    };

    // Define an offline solver.
    let offline_solver = solver::Offline::new(elm_home(), "0.19.1");

    // Define an online solver if needed.
    let remote = "https://package.elm-lang.org";
    let strat = online_strat.clone().unwrap_or(VersionStrategy::Newest);
    let mk_online_solver =
        |offline_solver| solver::Online::new(offline_solver, remote, http_fetch, strat).unwrap();

    let solution: AppDependencies = match (offline, online_strat) {
        (true, _) => {
            eprintln!("Solving offline");
            offline_solver.solve_deps(&project_elm_json).unwrap()
        }
        (false, None) => {
            eprintln!("Trying to solve offline first");
            offline_solver
                .solve_deps(&project_elm_json)
                .unwrap_or_else(|_| {
                    eprintln!("Offline solving failed, switching to online");
                    mk_online_solver(offline_solver)
                        .solve_deps(&project_elm_json)
                        .unwrap()
                })
        }
        (false, Some(_)) => {
            eprintln!("Solving online with strategy {:?}", &strat);
            mk_online_solver(offline_solver)
                .solve_deps(&project_elm_json)
                .unwrap()
        }
    };

    // Write solution to stdout.
    println!("{}", serde_json::to_string_pretty(&solution).unwrap());
}

// Helper functions ######################################################################

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
