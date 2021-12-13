use csv;
use pubgrub::solver::{resolve, OfflineDependencyProvider};
use pubgrub::version::SemanticVersion as SemVer;
use serde::Serialize;
use serde_json;
use std::io;
use std::str::FromStr;

use elm_solve_deps::pkg_version::PkgVersion;
use elm_solve_deps::project_config::PackageConfig;

/// Read the history of all packages and fetch all their elm.json files.
fn main() {
    let s = std::fs::read_to_string("registry/all-packages-history.json").expect("woops file");
    let raw: Vec<String> = serde_json::from_str(&s).expect("woops serde");
    let pkg_versions: Vec<PkgVersion> =
        raw.iter().map(|s| FromStr::from_str(&s).unwrap()).collect();
    let configs: Vec<PackageConfig> = pkg_versions
        .iter()
        .map(|p| p.load_from_cache("download").unwrap())
        .collect();

    let mut stats = Vec::new();
    for (id, conf) in configs.iter().enumerate() {
        stats.push(PkgStats {
            id,
            author: pkg_versions[id].author_pkg.author.clone(),
            pkg: conf.name.to_string(),
            version: conf.version,
            elm_version: conf.elm_version.0.lowest_version().unwrap(),
            license: conf.license.clone(),
            direct_dep_count: conf.dependencies.len(),
            total_dep_count: 0,
        });
    }

    let s = std::fs::read_to_string("registry/elm-packages.ron").unwrap();
    let deps_provider: OfflineDependencyProvider<String, SemVer> = ron::de::from_str(&s).unwrap();
    for stat in stats.iter_mut() {
        match resolve(&deps_provider, stat.pkg.clone(), stat.version.clone()) {
            Ok(all_deps) => stat.total_dep_count = all_deps.len() - 1,
            Err(_) => {}
        }
    }

    let mut wtr = csv::Writer::from_writer(io::stdout());
    stats
        .iter()
        .for_each(|record| wtr.serialize(record).unwrap());
    wtr.flush().unwrap();
}

#[derive(Debug, Clone, Serialize)]
struct PkgStats {
    id: usize,
    author: String,
    pkg: String,
    version: SemVer,
    elm_version: SemVer,
    license: String,
    direct_dep_count: usize,
    total_dep_count: usize,
}
