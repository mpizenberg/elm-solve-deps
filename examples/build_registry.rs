use pubgrub::solver::OfflineDependencyProvider;
use pubgrub::version::SemanticVersion as SemVer;
use serde_json;
use std::str::FromStr;

use pubgrub_dependency_provider_elm::pkg_version::PkgVersion;
use pubgrub_dependency_provider_elm::project_config::{PackageConfig, Pkg};

/// Read the history of all packages and fetch all their elm.json files.
fn main() {
    let s = std::fs::read_to_string("registry/all-packages-history.json").expect("woops file");
    let raw: Vec<String> = serde_json::from_str(&s).expect("woops serde");
    let pkg_versions: Vec<PkgVersion> =
        raw.iter().map(|s| FromStr::from_str(&s).unwrap()).collect();
    let http_fetch = |url: &str| {
        ureq::get(url)
            .timeout_connect(10_000)
            .call()
            .into_string()
            .map_err(|e| e.into())
    };
    let configs: Vec<PackageConfig> = pkg_versions
        .into_iter()
        // .skip(3772)
        // .take(2)
        .map(|p| {
            p.load_from_cache("download")
                .or_else(|_| p.fetch_config("download", "https://package.elm-lang.org", http_fetch))
                .unwrap()
        })
        .collect();
    let mut dep_provider: OfflineDependencyProvider<Pkg, SemVer> = OfflineDependencyProvider::new();
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 14, 0), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 14, 1), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 15, 0), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 15, 1), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 16, 0), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 16, 1), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 17, 0), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 17, 1), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 18, 0), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 19, 0), vec![]);
    dep_provider.add_dependencies(Pkg::new("elm", ""), (0, 19, 1), vec![]);
    configs.iter().for_each(|config| {
        let deps = config
            .dependencies_iter()
            .map(|(p, r)| (p.clone(), r.clone()))
            .chain(std::iter::once((
                Pkg::new("elm", ""),
                config.elm_version.0.clone(),
            )));
        dep_provider.add_dependencies(config.name.clone(), config.version.clone(), deps);
    });
    let pretty_config = ron::ser::PrettyConfig::new()
        .with_depth_limit(6)
        .with_indentor("  ".to_string());
    let registry = ron::ser::to_string_pretty(&dep_provider, pretty_config).expect("woops ron");
    // let registry = ron::ser::to_string(&dep_provider).expect("woops ron");
    std::fs::write("registry/elm-packages.ron", &registry).expect("woops ron write");
}
