use std::path::PathBuf;
use std::str::FromStr;
use std::{error::Error, process::exit};

use dirs;
use ureq;

use pubgrub_dependency_provider_elm::constraint::Constraint;
use pubgrub_dependency_provider_elm::pkg_version::PkgVersion;
use pubgrub_dependency_provider_elm::project_config::{AppDependencies, Pkg, ProjectConfig};
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
        solve_deps --extra "elm/json: 1.1.3 <= v < 2.0.0"

FLAGS:
    --help                 Print this message and exit
    --offline              No network request, use only installed packages
    --online-newest        Use the newest compatible version
    --online-oldest        Use the oldest compatible version
    --extra "author/package: constraint"
                           Additional package version constraint
                           Need one --extra per additional constraint
                           MUST be placed before an eventual package to solve
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_option = |s: &&str| s.starts_with("--");
    let (options, positional): (Vec<&str>, Vec<&str>) =
        args.iter().map(|s| s.as_str()).partition(is_option);

    // Check for the --help option
    if options.contains(&"--help") {
        println!("{}", HELP);
        exit(0);
    }

    // Check for connectivity and strategy
    let offline = options.contains(&"--offline");
    let mut online_strat = None;
    if options.contains(&"--online-newest") {
        online_strat = Some(VersionStrategy::Newest);
    } else if options.contains(&"--online-oldest") {
        online_strat = Some(VersionStrategy::Oldest);
    }

    // Check for extra additional constraints
    let extra_count = options.iter().filter(|&o| o == &"--extra").count();
    let (extras_args, pkg) = positional.split_at(extra_count);
    let extras: Vec<(Pkg, Constraint)> = extras_args
        .iter()
        .map(|s| {
            let (pkg_str, range_str) = s.split_once(':').unwrap();
            (
                Pkg::from_str(pkg_str.trim()).unwrap(),
                Constraint::from_str(range_str.trim()).unwrap(),
            )
        })
        .collect();

    let maybe_pkg_version = pkg.get(0).map(|p| PkgVersion::from_str(p).unwrap());
    run(maybe_pkg_version, offline, online_strat, &extras);
}

fn run(
    maybe_pkg_version: Option<PkgVersion>,
    offline: bool,
    online_strat: Option<VersionStrategy>,
    extras: &[(Pkg, Constraint)],
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
            offline_solver
                .solve_deps(&project_elm_json, extras)
                .unwrap()
        }
        (false, None) => {
            eprintln!("Trying to solve offline first");
            offline_solver
                .solve_deps(&project_elm_json, extras)
                .unwrap_or_else(|_| {
                    eprintln!("Offline solving failed, switching to online");
                    mk_online_solver(offline_solver)
                        .solve_deps(&project_elm_json, extras)
                        .unwrap()
                })
        }
        (false, Some(_)) => {
            eprintln!("Solving online with strategy {:?}", &strat);
            mk_online_solver(offline_solver)
                .solve_deps(&project_elm_json, extras)
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
