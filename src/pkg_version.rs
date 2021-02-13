use pubgrub::version::{SemanticVersion as SemVer, VersionParseError};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

use crate::project_config::{PackageConfig, Pkg, PkgParseError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cache {
    pub cache: BTreeMap<Pkg, BTreeSet<SemVer>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkgVersion {
    pub author_pkg: Pkg,
    pub version: SemVer,
}

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("unable to read/write cache")]
    FileIoError(#[from] std::io::Error),
    #[error("failed to parse/convert JSON")]
    JsonError(#[from] serde_json::Error),
    #[error("failed to fetch {url:?}")]
    FetchError {
        url: String,
        #[source]
        source: Box<dyn std::error::Error>,
    },
    #[error("failed parse package version")]
    PkgVersionFromStrError(#[from] PkgVersionError),
}

#[derive(Error, Debug)]
pub enum PkgVersionError {
    #[error("unable to read/write cache")]
    FileIoError(#[from] std::io::Error),
    #[error("failed to parse/convert JSON")]
    JsonError(#[from] serde_json::Error),
    #[error("failed to fetch {url:?}")]
    FetchError {
        url: String,
        #[source]
        source: Box<dyn std::error::Error>,
    },
    #[error("failed to parse")]
    ParseError(#[from] PkgVersionParseError),
}

#[derive(Error, Debug)]
pub enum PkgVersionParseError {
    #[error("no package@version separation found in `{0}`")]
    NoVersionSeparator(String),
    #[error("failed to parse version in `{0}`")]
    VersionParseError(#[from] VersionParseError),
    #[error("failed to parse the package")]
    PkgParseError(#[from] PkgParseError),
}

impl Cache {
    /// Initialize an empty cache.
    pub fn new() -> Self {
        Self {
            cache: BTreeMap::new(),
        }
    }

    /// List installed versions in ~/.elm/.
    pub fn list_installed_versions<P: AsRef<Path>>(
        elm_home: P,
        elm_version: &str,
        author_pkg: &Pkg,
    ) -> Result<BTreeSet<SemVer>, PkgParseError> {
        let p_dir = author_pkg.config_path(elm_home, elm_version);
        let sub_dirs = match std::fs::read_dir(&p_dir) {
            Ok(s) => s,
            Err(_) => {
                // The directory does not exist so probably
                // no version of this package have ever been installed.
                return Ok(BTreeSet::new());
            }
        };

        // List installed versions
        Ok(sub_dirs
            .filter_map(|f| f.ok())
            // only keep directories
            .filter(|entry| entry.file_type().map(|f| f.is_dir()).unwrap_or(false))
            // retrieve the directory name as a string
            .filter_map(|entry| entry.file_name().into_string().ok())
            // convert into a version
            .filter_map(|s| SemVer::from_str(&s).ok())
            .collect())
    }

    /// Load the cache from its default location.
    pub fn load<P: AsRef<Path>>(elm_home: P) -> Result<Self, CacheError> {
        // eprintln!(
        //     "Loading versions cache from {}",
        //     Self::file_path(&elm_home).display()
        // );
        let s = std::fs::read_to_string(Self::file_path(elm_home))?;
        serde_json::from_str(&s).map_err(|e| e.into())
    }

    /// Save the cache to its default location.
    pub fn save<P: AsRef<Path>>(&self, elm_home: P) -> Result<(), CacheError> {
        // eprintln!(
        //     "Saving versions cache into {}",
        //     Self::file_path(&elm_home).display()
        // );
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
        http_fetch: impl Fn(&str) -> Result<String, Box<dyn std::error::Error>>,
    ) -> Result<(), CacheError> {
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
            // eprintln!("Request to {}", url);
            let pkgs_str =
                http_fetch(&url).map_err(|e| CacheError::FetchError { url, source: e })?;
            let new_versions_str: Vec<&str> = serde_json::from_str(&pkgs_str)?;
            if new_versions_str.is_empty() {
                // Reload from scratch since it means a package was deleted from the registry
                // and no new package showed up
                *self = Self::from_remote_all_pkg(remote_base_url, http_fetch)?;
                return Ok(());
            }
            // Check that the last package in the list was already in cache
            // (the list returned by the package server is sorted newest first)
            let (last, newers) = new_versions_str.split_last().unwrap(); // This unwrap is fine since we checked that new_versions_str is not empty
            let last_pkg = PkgVersion::from_str(last).map_err(PkgVersionError::ParseError)?;
            if self
                .cache
                .get(&last_pkg.author_pkg)
                .and_then(|pkg_versions| pkg_versions.get(&last_pkg.version))
                .is_some()
            {
                // Continue as normal: register every new package version
                for version_str in &newers[..] {
                    let PkgVersion {
                        author_pkg,
                        version,
                    } = PkgVersion::from_str(version_str).map_err(PkgVersionError::ParseError)?;
                    let pkg_entry = self.cache.entry(author_pkg).or_default();
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
        http_fetch: impl Fn(&str) -> Result<String, Box<dyn std::error::Error>>,
    ) -> Result<Self, CacheError> {
        let url = format!("{}/all-packages", remote_base_url);
        // eprintln!("Request to {}", url);
        let all_pkg_str =
            http_fetch(&url).map_err(|e| CacheError::FetchError { url, source: e })?;
        serde_json::from_str(&all_pkg_str).map_err(|e| e.into())
    }
}

// Public PkgVersion methods.
impl PkgVersion {
    pub fn fetch_config<P: AsRef<Path>>(
        &self,
        elm_home: P,
        remote_base_url: &str,
        http_fetch: impl Fn(&str) -> Result<String, Box<dyn std::error::Error>>,
    ) -> Result<PackageConfig, PkgVersionError> {
        let remote_url = self.to_url(remote_base_url);
        // eprintln!("Fetching {}", &remote_url);
        let config_str = http_fetch(&remote_url).map_err(|e| PkgVersionError::FetchError {
            url: remote_url,
            source: e,
        })?;
        std::fs::create_dir_all(self.pubgrub_cache_dir(&elm_home))?;
        std::fs::write(self.pubgrub_cache_file(&elm_home), &config_str)?;
        let config = serde_json::from_str(&config_str)?;
        Ok(config)
    }

    pub fn load_config<P: AsRef<Path>>(
        &self,
        elm_home: P,
        elm_version: &str,
    ) -> Result<PackageConfig, PkgVersionError> {
        let config_path = self.config_path(elm_home, elm_version);
        // eprintln!("Loading {:?}", &config_path);
        let config_str = std::fs::read_to_string(&config_path)?;
        let config = serde_json::from_str(&config_str)?;
        Ok(config)
    }

    pub fn load_from_cache<P: AsRef<Path>>(
        &self,
        elm_home: P,
    ) -> Result<PackageConfig, PkgVersionError> {
        let cache_path = self.pubgrub_cache_file(elm_home);
        // eprintln!("Cache-loading {:?}", &cache_path);
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

impl FromStr for PkgVersion {
    type Err = PkgVersionParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let version_sep = s
            .find('@')
            .ok_or_else(|| PkgVersionParseError::NoVersionSeparator(s.to_string()))?;
        let author_pkg = Pkg::from_str(&s[0..version_sep])?;
        let version = FromStr::from_str(&s[(version_sep + 1)..])?;
        Ok(PkgVersion {
            author_pkg,
            version,
        })
    }
}
