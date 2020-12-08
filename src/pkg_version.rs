use pubgrub::version::{SemanticVersion as SemVer, VersionParseError};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::project_config::PackageConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cache {
    pub cache: BTreeMap<String, BTreeSet<SemVer>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkgVersion {
    pub author_pkg: Pkg,
    pub version: SemVer,
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
pub struct Pkg {
    pub author: String,
    pub pkg: String,
}

impl Cache {
    /// Initialize an empty cache.
    pub fn new() -> Self {
        Self {
            cache: BTreeMap::new(),
        }
    }

    /// Load the cache from its default location.
    pub fn load<P: AsRef<Path>>(elm_home: P) -> Result<Self, Box<dyn Error>> {
        eprintln!(
            "Loading versions cache from {}",
            Self::file_path(&elm_home).display()
        );
        let s = std::fs::read_to_string(Self::file_path(elm_home))?;
        serde_json::from_str(&s).map_err(|e| e.into())
    }

    /// Save the cache to its default location.
    pub fn save<P: AsRef<Path>>(&self, elm_home: P) -> Result<(), Box<dyn Error>> {
        eprintln!(
            "Saving versions cache into {}",
            Self::file_path(&elm_home).display()
        );
        let s = serde_json::to_string(self)?;
        std::fs::write(Self::file_path(elm_home), &s).map_err(|e| e.into())
    }

    /// Path the to file used to store a cache of all existing versions.
    /// ~/.elm/pubgrub/versions_cache.json
    pub fn file_path<P: AsRef<Path>>(elm_home: P) -> PathBuf {
        Pkg::pubgrub_cache_dir(elm_home).join("versions_cache.json")
    }

    /// Fetch packages online.
    pub fn update(
        &mut self,
        remote_base_url: &str,
        http_fetch: impl Fn(&str) -> Result<String, Box<dyn Error>>,
    ) -> Result<(), Box<dyn Error>> {
        if self.cache.is_empty() {
            *self = Self::from_remote_all_pkg(remote_base_url, http_fetch)?;
            Ok(())
        } else {
            let versions_count: usize = self.cache.values().map(|v| v.len()).sum();
            let url = format!(
                "{}/all-packages/since/{}",
                remote_base_url,
                versions_count.max(1) - 1
            );
            eprintln!("Request to {}", url);
            let pkgs_str = http_fetch(&url)?;
            let new_versions_str: Vec<&str> = serde_json::from_str(&pkgs_str)?;
            if new_versions_str.is_empty() {
                // Reload from scratch since it means a package was deleted from the registry
                // and no new package showed up
                *self = Self::from_remote_all_pkg(remote_base_url, http_fetch)?;
                return Ok(());
            }
            // Check that the last package in the list was already in cache
            // (the list returned by the package server is sorted newest first)
            let (last, newers) = new_versions_str.split_last().unwrap();
            let last_pkg = PkgVersion::from_str(last).unwrap();
            if self
                .cache
                .get(&last_pkg.author_pkg.to_string())
                .and_then(|pkg_versions| pkg_versions.get(&last_pkg.version))
                .is_some()
            {
                // Continue as normal: register every new package version
                for version_str in &newers[..] {
                    let PkgVersion {
                        author_pkg,
                        version,
                    } = PkgVersion::from_str(version_str).unwrap();
                    let pkg_entry = self.cache.entry(author_pkg.to_string()).or_default();
                    pkg_entry.insert(version);
                }
            } else {
                // Reload from scratch since it means a package was deleted from the registry
                *self = Self::from_remote_all_pkg(remote_base_url, http_fetch)?;
            }
            Ok(())
        }
    }

    /// curl -L https://package.elm-lang.org/all-packages | jq .
    fn from_remote_all_pkg(
        remote_base_url: &str,
        http_fetch: impl Fn(&str) -> Result<String, Box<dyn Error>>,
    ) -> Result<Self, Box<dyn Error>> {
        let url = format!("{}/all-packages", remote_base_url);
        eprintln!("Request to {}", url);
        let all_pkg_str = http_fetch(&url)?;
        serde_json::from_str(&all_pkg_str).map_err(|e| e.into())
    }
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
            .pubgrub_cache_dir_json(elm_home)
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
        Self::packages_dir(elm_home, elm_version)
            .join(&self.author)
            .join(&self.pkg)
    }
}

// Private Pkg methods.
impl Pkg {
    fn to_url(&self, remote_base_url: &str) -> String {
        format!("{}/packages/{}/{}", remote_base_url, self.author, self.pkg)
    }

    fn pubgrub_cache_dir_json<P: AsRef<Path>>(&self, elm_home: P) -> PathBuf {
        Self::pubgrub_cache_dir(elm_home)
            .join("elm_json_cache")
            .join(&self.author)
            .join(&self.pkg)
    }

    fn pubgrub_cache_dir<P: AsRef<Path>>(elm_home: P) -> PathBuf {
        elm_home.as_ref().join("pubgrub")
    }

    fn packages_dir<P: AsRef<Path>>(elm_home: P, elm_version: &str) -> PathBuf {
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

impl fmt::Display for Pkg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", &self.author, &self.pkg)
    }
}
