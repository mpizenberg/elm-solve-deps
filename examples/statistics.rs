use csv;
use pubgrub::solver::{resolve, OfflineDependencyProvider};
use pubgrub::version::{SemanticVersion as SemVer, VersionParseError};
use pubgrub_dependency_provider_elm::project_config::PackageConfig;
use serde::Serialize;
use serde_json;
// use std::collections::BTreeSet as Set;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;

/// Read the history of all packages and fetch all their elm.json files.
fn main() {
    let s = std::fs::read_to_string("registry/all-packages-history.json").expect("woops file");
    let raw: Vec<String> = serde_json::from_str(&s).expect("woops serde");
    let pkg_versions: Vec<PkgVersion> =
        raw.iter().map(|s| FromStr::from_str(&s).unwrap()).collect();
    let configs: Vec<PackageConfig> = pkg_versions
        .iter()
        .map(|p| p.load_config().unwrap())
        .collect();

    let mut stats = Vec::new();
    for (id, conf) in configs.iter().enumerate() {
        stats.push(PkgStats {
            id,
            author: pkg_versions[id].user.clone(),
            pkg: conf.name.clone(),
            version: conf.version,
            elm_version: conf.elm_version.0.lowest_version().unwrap(),
            license: conf.license.clone(),
            direct_dep_count: conf.dependencies.len(),
            total_dep_count: 0,
        });
    }

    let s = std::fs::read_to_string("elm-packages.ron").unwrap();
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

#[derive(Debug, Clone)]
struct PkgVersion {
    user: String,
    pkg: String,
    version: SemVer,
}

impl PkgVersion {
    pub fn load_config(&self) -> Result<PackageConfig, Box<dyn Error>> {
        eprintln!("Loading {:?}", self.config_path());
        let config_str = std::fs::read_to_string(self.config_path())?;
        let config = serde_json::from_str(&config_str)?;
        Ok(config)
    }

    fn config_dir(&self) -> PathBuf {
        let mut path = PathBuf::new();
        path.push("download");
        path.push(&self.user);
        path.push(&self.pkg);
        path.push(&self.version.to_string());
        path
    }

    fn config_path(&self) -> PathBuf {
        let mut path = self.config_dir();
        path.push("elm.json");
        path
    }
}

impl FromStr for PkgVersion {
    type Err = VersionParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let name_sep = s.find('/').expect("Invalid pkg: no name sep");
        let user = s[0..name_sep].to_string();
        let version_sep = s.find('@').expect("Invalid pkg: no version sep");
        let pkg = s[(name_sep + 1)..version_sep].to_string();
        let version = FromStr::from_str(&s[(version_sep + 1)..])?;
        Ok(PkgVersion { user, pkg, version })
    }
}
