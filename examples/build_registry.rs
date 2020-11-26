use pubgrub::solver::OfflineDependencyProvider;
use pubgrub::version::{SemanticVersion as SemVer, VersionParseError};
use pubgrub_dependency_provider_elm::project_config::PackageConfig;
use serde_json;
// use std::collections::BTreeSet as Set;
use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;
use ureq;

/// Read the history of all packages and fetch all their elm.json files.
fn main() {
    let s = std::fs::read_to_string("registry/all-packages-history.json").expect("woops file");
    let raw: Vec<String> = serde_json::from_str(&s).expect("woops serde");
    let pkg_versions: Vec<PkgVersion> =
        raw.iter().map(|s| FromStr::from_str(&s).unwrap()).collect();
    let configs: Vec<PackageConfig> = pkg_versions
        .into_iter()
        // .skip(3772)
        // .take(2)
        // .map(|p| p.fetch_config().unwrap())
        .map(|p| p.load_config().unwrap())
        .collect();
    let mut dep_provider: OfflineDependencyProvider<String, SemVer> =
        OfflineDependencyProvider::new();
    dep_provider.add_dependencies("elm".to_string(), (0, 14, 0), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 14, 1), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 15, 0), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 15, 1), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 16, 0), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 16, 1), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 17, 0), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 17, 1), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 18, 0), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 19, 0), vec![]);
    dep_provider.add_dependencies("elm".to_string(), (0, 19, 1), vec![]);
    configs.iter().for_each(|config| {
        let deps = config
            .dependencies
            .iter()
            .map(|(p, c)| (p.clone(), c.0.clone()))
            .chain(std::iter::once((
                "elm".to_string(),
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

#[derive(Debug, Clone)]
struct PkgVersion {
    user: String,
    pkg: String,
    version: SemVer,
}

// Public methods.
impl PkgVersion {
    pub fn fetch_config(&self) -> Result<PackageConfig, Box<dyn Error>> {
        eprintln!("Fetching {}", self.to_url());
        let response = ureq::get(&self.to_url()).timeout_connect(10_000).call();
        let config_str = response.into_string()?;
        std::fs::create_dir_all(self.config_dir())?;
        std::fs::write(self.config_path(), &config_str)?;
        let config = serde_json::from_str(&config_str)?;
        Ok(config)
    }

    pub fn load_config(&self) -> Result<PackageConfig, Box<dyn Error>> {
        eprintln!("Loading {:?}", self.config_path());
        let config_str = std::fs::read_to_string(self.config_path())?;
        let config = serde_json::from_str(&config_str)?;
        Ok(config)
    }
}

// Private methods.
impl PkgVersion {
    fn to_url(&self) -> String {
        format!(
            "https://package.elm-lang.org/packages/{}/{}/{}/elm.json",
            self.user, self.pkg, self.version
        )
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
