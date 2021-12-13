use std::path::PathBuf;
use std::str::FromStr;
use std::{error::Error, process::exit};

use anyhow::Context;
use pubgrub::error::PubGrubError;
use pubgrub::report::{DefaultStringReporter, Reporter};
use pubgrub::version::SemanticVersion as SemVer;

use elm_solve_deps::constraint::Constraint;
use elm_solve_deps::pkg_version::PkgVersion;
use elm_solve_deps::project_config::{AppDependencies, Pkg, ProjectConfig};
use elm_solve_deps::solver::{self, VersionStrategy};

const HELP: &str = r#"
solve-deps-bin

Solve dependencies of an Elm project or published package.
By default, try in offline mode first
and switch to online mode if that fails.

USAGE:
    solve-deps-bin [FLAGS...] [author/package@version]
    For example:
        solve-deps-bin
        solve-deps-bin --help
        solve-deps-bin --offline
        solve-deps-bin ianmackenzie/elm-3d-scene@1.0.1
        solve-deps-bin --offline jxxcarlson/elm-tar@4.0.0
        solve-deps-bin --online-newest w0rm/elm-physics@5.1.1
        solve-deps-bin --online-oldest lucamug/style-framework@1.1.0
        solve-deps-bin --test
        solve-deps-bin --extra "elm/json: 1.1.3 <= v < 2.0.0"

FLAGS:
    --help                 Print this message and exit
    --offline              No network request, use only installed packages
    --online-newest        Use the newest compatible version
    --online-oldest        Use the oldest compatible version
    --test                 Solve with both normal and test dependencies
    --extra "author/package: constraint"
                           Additional package version constraint
                           Need one --extra per additional constraint
                           MUST be placed before an eventual package to solve
"#;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_option = |s: &&str| s.starts_with("--");
    let (options, positional): (Vec<&str>, Vec<&str>) =
        args.iter().map(|s| s.as_str()).partition(is_option);

    // Check for the --help option
    if options.contains(&"--help") {
        println!("{}", HELP);
        exit(0);
    }

    // Check if solving with test dependencies
    let use_test = options.contains(&"--test");

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
    let parse_package_constraint = |s: &&str| {
        let (pkg_str, range_str) = s.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "Did not find the separator ':' in the extra argument {}",
                s.to_string()
            )
        })?;
        Ok((
            Pkg::from_str(pkg_str.trim())?,
            Constraint::from_str(range_str.trim())?,
        ))
    };
    let extras: anyhow::Result<Vec<(Pkg, Constraint)>> =
        extras_args.iter().map(parse_package_constraint).collect();

    let maybe_pkg_version = match pkg.get(0) {
        Some(p_str) => Some(PkgVersion::from_str(p_str).context(format!(
            "Failed to parse the package to solve: {}",
            p_str.to_string(),
        ))?),
        None => None,
    };
    run(maybe_pkg_version, offline, online_strat, use_test, &extras?)
}

fn run(
    maybe_pkg_version: Option<PkgVersion>,
    offline: bool,
    online_strat: Option<VersionStrategy>,
    use_test: bool,
    extras: &[(Pkg, Constraint)],
) -> anyhow::Result<()> {
    let elm_version = "0.19.1";

    // Load the elm.json of the package given as argument or of the current folder.
    let project_elm_json: ProjectConfig = match maybe_pkg_version {
        Some(pkg_version) => {
            let pkg_config = pkg_version
                .load_config(elm_home(), elm_version)
                .or_else(|_| pkg_version.load_from_cache(elm_home()))
                .or_else(|_| {
                    pkg_version.fetch_config(elm_home(), "https://package.elm-lang.org", http_fetch)
                })
                .context("Failed to load the elm.json config of the package to solve")?;
            ProjectConfig::Package(pkg_config)
        }
        None => {
            let elm_json_str = std::fs::read_to_string("elm.json")
                .context("Are you in an elm project? there was an issue loading the elm.json")?;
            serde_json::from_str(&elm_json_str).context("Failed to decode the elm.json")?
        }
    };

    // Define an offline solver.
    let offline_solver = solver::Offline::new(elm_home(), "0.19.1");

    // Define an online solver if needed.
    let remote = "https://package.elm-lang.org";
    let strat = online_strat.unwrap_or(VersionStrategy::Newest);
    let mk_online_solver =
        |offline_solver| solver::Online::new(offline_solver, remote, http_fetch, strat);

    let solution: AppDependencies = match (offline, online_strat) {
        (true, _) => {
            eprintln!("Solving offline");
            offline_solver
                .solve_deps(&project_elm_json, use_test, extras)
                .map_err(handle_pubgrub_error)?
        }
        (false, None) => {
            eprintln!("Trying to solve offline first");
            offline_solver
                .solve_deps(&project_elm_json, use_test, extras)
                .or_else(|_| {
                    eprintln!("Offline solving failed, switching to online");
                    mk_online_solver(offline_solver)
                        .context("Failed to initialize the online solver")?
                        .solve_deps(&project_elm_json, use_test, extras)
                        .map_err(handle_pubgrub_error)
                })?
        }
        (false, Some(_)) => {
            eprintln!("Solving online with strategy {:?}", &strat);
            mk_online_solver(offline_solver)
                .context("Failed to initialize the online solver")?
                .solve_deps(&project_elm_json, use_test, extras)
                .map_err(handle_pubgrub_error)?
        }
    };

    // Write solution to stdout.
    println!("{}", serde_json::to_string_pretty(&solution)?);
    Ok(())
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

fn http_fetch(url: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
    ureq::get(url)
        .timeout_connect(10_000)
        .call()
        .into_string()
        .map_err(|e| e.into())
}

fn handle_pubgrub_error(err: PubGrubError<Pkg, SemVer>) -> anyhow::Error {
    match err {
        PubGrubError::NoSolution(tree) => {
            anyhow::anyhow!(DefaultStringReporter::report(&tree))
        }
        PubGrubError::ErrorRetrievingDependencies {
            package,
            version,
            source,
        } => anyhow::anyhow!(
            "An error occured while trying to retrieve dependencies of {}@{}:\n\n{}",
            package,
            version,
            source
        ),
        PubGrubError::DependencyOnTheEmptySet {
            package,
            version,
            dependent,
        } => anyhow::anyhow!(
            "{}@{} has an imposible dependency on {}",
            package,
            version,
            dependent
        ),
        PubGrubError::SelfDependency { package, version } => {
            anyhow::anyhow!("{}@{} somehow depends on itself", package, version)
        }
        PubGrubError::ErrorChoosingPackageVersion(err) => anyhow::anyhow!(
            "There was an error while picking packages for dependency resolution:\n\n{}",
            err
        ),
        PubGrubError::ErrorInShouldCancel(err) => {
            anyhow::anyhow!("Dependency resolution was cancelled.\n\n{}", err)
        }
        PubGrubError::Failure(err) => anyhow::anyhow!(
            "An unrecoverable error happened while solving dependencies:\n\n{}",
            err
        ),
    }
}
