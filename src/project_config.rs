//! Module dealing with project configuration in the elm.json file.

use crate::constraint::Constraint;
use pubgrub::range::Range;
use pubgrub::version::SemanticVersion as SemVer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap as Map;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

/// Project configuration in an elm.json.
/// It either is a package or an application.
/// Both have different sets of fields.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProjectConfig {
    Application(ApplicationConfig),
    Package(PackageConfig),
}

/// Struct representing an application elm.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApplicationConfig {
    pub source_directories: Vec<String>,
    pub elm_version: SemVer,
    pub dependencies: AppDependencies,
    pub test_dependencies: AppDependencies,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDependencies {
    pub direct: Map<Pkg, SemVer>,
    pub indirect: Map<Pkg, SemVer>,
}

/// Struct representing a package elm.json.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PackageConfig {
    pub name: String,
    pub summary: String,
    pub license: String,
    pub version: SemVer,
    pub elm_version: Constraint,
    pub exposed_modules: ExposedModules,
    pub dependencies: Map<Pkg, Constraint>,
    pub test_dependencies: Map<Pkg, Constraint>,
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Pkg {
    pub author: String,
    pub pkg: String,
}

#[derive(Error, Debug)]
pub enum PkgParseError {
    #[error("no author/package separation found in `{0}`")]
    NoAuthorSeparator(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExposedModules {
    NoCategory(Vec<String>),
    WithCategories(Map<String, Vec<String>>),
}

impl PackageConfig {
    pub fn dependencies_iter(&self) -> impl Iterator<Item = (&Pkg, &Range<SemVer>)> {
        self.dependencies
            .iter()
            .map(|(p, constraint)| (p, &constraint.0))
    }
}

// Public Pkg methods.
impl Pkg {
    pub fn new(author: String, pkg: String) -> Self {
        Self { author, pkg }
    }

    pub fn config_path<P: AsRef<Path>>(&self, elm_home: P, elm_version: &str) -> PathBuf {
        Self::packages_dir(elm_home, elm_version)
            .join(&self.author)
            .join(&self.pkg)
    }

    pub fn pubgrub_cache_dir<P: AsRef<Path>>(elm_home: P) -> PathBuf {
        elm_home.as_ref().join("pubgrub")
    }

    pub fn to_url(&self, remote_base_url: &str) -> String {
        format!("{}/packages/{}/{}", remote_base_url, self.author, self.pkg)
    }

    pub fn pubgrub_cache_dir_json<P: AsRef<Path>>(&self, elm_home: P) -> PathBuf {
        Self::pubgrub_cache_dir(elm_home)
            .join("elm_json_cache")
            .join(&self.author)
            .join(&self.pkg)
    }
}

// Private Pkg methods.
impl Pkg {
    fn packages_dir<P: AsRef<Path>>(elm_home: P, elm_version: &str) -> PathBuf {
        elm_home.as_ref().join(elm_version).join("packages")
    }
}

impl FromStr for Pkg {
    type Err = PkgParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let author_sep = s
            .find('/')
            .ok_or_else(|| PkgParseError::NoAuthorSeparator(s.to_string()))?;
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
