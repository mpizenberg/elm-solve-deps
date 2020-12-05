use pubgrub::version::{SemanticVersion as SemVer, VersionParseError};
use serde_json;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::project_config::PackageConfig;

#[derive(Debug, Clone)]
pub struct PkgVersion {
    pub author_pkg: Pkg,
    pub version: SemVer,
}

#[derive(Debug, Clone)]
pub struct Pkg {
    pub author: String,
    pub pkg: String,
}

// Public PkgVersion methods.
impl PkgVersion {
    pub fn fetch_config<P: AsRef<Path>>(
        &self,
        elm_home: P,
        remote_base_url: &str,
        http_fetch: impl Fn(&str) -> Result<String, Box<dyn Error>>,
    ) -> Result<PackageConfig, Box<dyn Error>> {
        eprintln!("Fetching {}", self.to_url(remote_base_url));
        let config_str = http_fetch(&self.to_url(remote_base_url))?;
        std::fs::create_dir_all(self.pubgrub_cache_dir(&elm_home))?;
        std::fs::write(self.pubgrub_cache_file(&elm_home), &config_str)?;
        let config = serde_json::from_str(&config_str)?;
        Ok(config)
    }

    pub fn load_config<P: AsRef<Path>>(
        &self,
        elm_home: P,
        elm_version: &str,
    ) -> Result<PackageConfig, Box<dyn Error>> {
        let config_path = self.config_path(elm_home, elm_version);
        eprintln!("Loading {:?}", &config_path);
        let config_str = std::fs::read_to_string(&config_path)?;
        let config = serde_json::from_str(&config_str)?;
        Ok(config)
    }

    pub fn load_from_cache<P: AsRef<Path>>(
        &self,
        elm_home: P,
    ) -> Result<PackageConfig, Box<dyn Error>> {
        let cache_path = self.pubgrub_cache_file(elm_home);
        eprintln!("Cache-loading {:?}", &cache_path);
        let config_str = std::fs::read_to_string(&cache_path)?;
        let config = serde_json::from_str(&config_str)?;
        Ok(config)
    }
}

// Private PkgVersion methods.
impl PkgVersion {
    fn to_url(&self, remote_base_url: &str) -> String {
        format!(
            "{}/{}/elm.json",
            self.author_pkg.to_url(remote_base_url),
            self.version
        )
    }

    fn pubgrub_cache_file<P: AsRef<Path>>(&self, elm_home: P) -> PathBuf {
        self.pubgrub_cache_dir(elm_home).join("elm.json")
    }

    fn pubgrub_cache_dir<P: AsRef<Path>>(&self, elm_home: P) -> PathBuf {
        self.author_pkg
            .pubgrub_cache_dir(elm_home)
            .join(&self.version.to_string())
    }

    fn config_path<P: AsRef<Path>>(&self, elm_home: P, elm_version: &str) -> PathBuf {
        self.author_pkg
            .config_path(elm_home, elm_version)
            .join(&self.version.to_string())
            .join("elm.json")
    }
}

// Public Pkg methods.
impl Pkg {
    pub fn config_path<P: AsRef<Path>>(&self, elm_home: P, elm_version: &str) -> PathBuf {
        self.packages_dir(elm_home, elm_version)
            .join(&self.author)
            .join(&self.pkg)
    }
}

// Private Pkg methods.
impl Pkg {
    fn to_url(&self, remote_base_url: &str) -> String {
        format!("{}/packages/{}/{}", remote_base_url, self.author, self.pkg)
    }

    fn pubgrub_cache_dir<P: AsRef<Path>>(&self, elm_home: P) -> PathBuf {
        elm_home
            .as_ref()
            .join("pubgrub")
            .join("elm_json_cache")
            .join(&self.author)
            .join(&self.pkg)
    }

    fn packages_dir<P: AsRef<Path>>(&self, elm_home: P, elm_version: &str) -> PathBuf {
        elm_home.as_ref().join(elm_version).join("packages")
    }
}

impl FromStr for PkgVersion {
    type Err = VersionParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let version_sep = s.find('@').expect("Invalid pkg: no version sep");
        let author_pkg = Pkg::from_str(&s[0..version_sep]).expect("Invalid pkg: no author sep");
        let version = FromStr::from_str(&s[(version_sep + 1)..])?;
        Ok(PkgVersion {
            author_pkg,
            version,
        })
    }
}

impl FromStr for Pkg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let author_sep = s.find('/').expect("Invalid pkg: no author sep");
        let author = s[0..author_sep].to_string();
        let pkg = s[(author_sep + 1)..].to_string();
        Ok(Pkg { author, pkg })
    }
}
