// SPDX-License-Identifier: MPL-2.0

//! Module dealing with project configuration related to the `elm.json` file.

use crate::constraint::Constraint;
use pubgrub::range::Range;
use pubgrub::version::SemanticVersion as SemVer;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap as Map;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

/// Project configuration corresponding to an `elm.json` file.
/// It either is a package or an application.
/// Both have different sets of fields.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProjectConfig {
    /// Application variant of a project config.
    Application(ApplicationConfig),
    /// Package variant of a project config.
    Package(PackageConfig),
}

/// Struct representing the `elm.json` of an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApplicationConfig {
    /// Source directories.
    pub source_directories: Vec<String>,
    /// Elm version.
    pub elm_version: SemVer,
    /// Dependencies of the application.
    pub dependencies: AppDependencies,
    /// Test dependencies of the application.
    pub test_dependencies: AppDependencies,
}

/// Dependencies of an elm application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDependencies {
    /// Direct dependencies.
    pub direct: Map<Pkg, SemVer>,
    /// Indirect dependencies.
    pub indirect: Map<Pkg, SemVer>,
}

/// Struct representing the `elm.json` of a package.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PackageConfig {
    /// Package identifier (author + package name).
    pub name: Pkg,
    /// Summary explanation of the package.
    pub summary: String,
    /// License of the package.
    pub license: String,
    /// Version of the package.
    pub version: SemVer,
    /// Version of elm that is compatible with this package.
    pub elm_version: Constraint,
    /// Exposed modules of the package.
    pub exposed_modules: ExposedModules,
    /// Dependencies of the package.
    pub dependencies: Map<Pkg, Constraint>,
    /// Test dependencies of the package.
    pub test_dependencies: Map<Pkg, Constraint>,
}

/// A package identifier, composed of the author name and the package name.
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Pkg {
    /// Author of the package.
    pub author: String,
    /// Package name.
    pub pkg: String,
}

/// Error type for parsing errors of package identifiers.
#[derive(Error, Debug)]
pub enum PkgParseError {
    /// Error corresponding to a missing separator between the author and package name.
    #[error("no author/package separation found in `{0}`")]
    NoAuthorSeparator(String),
}

/// Exposed modules, potentially regrouped by categories.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExposedModules {
    /// All modules are exposed at the same hierarchy.
    NoCategory(Vec<String>),
    /// Exposed modules are grouped by categories.
    WithCategories(Map<String, Vec<String>>),
}

impl PackageConfig {
    /// Generate an iterator over a package dependencies.
    pub fn dependencies_iter(&self) -> impl Iterator<Item = (&Pkg, &Range<SemVer>)> {
        self.dependencies
            .iter()
            .map(|(p, constraint)| (p, &constraint.0))
    }
}

// Public Pkg methods.
impl Pkg {
    /// Create a new package identifier from its two components, author and package name.
    pub fn new<S1: ToString, S2: ToString>(author: S1, pkg: S2) -> Self {
        Self {
            author: author.to_string(),
            pkg: pkg.to_string(),
        }
    }

    /// Get the location of the cache directory for the dependency solver.
    ///
    /// TODO: why is this function here?
    pub fn pubgrub_cache_dir<P: AsRef<Path>>(elm_home: P) -> PathBuf {
        elm_home.as_ref().join("pubgrub")
    }

    /// Get the path to the folder inside `ELM_HOME` containing the different installed versions of this package.
    pub fn config_path<P: AsRef<Path>>(&self, elm_home: P, elm_version: &str) -> PathBuf {
        Self::packages_dir(elm_home, elm_version)
            .join(&self.author)
            .join(&self.pkg)
    }

    /// Get the url corresponding to this package on the package server.
    ///
    /// This looks like `https://remote/packages/author/package`.
    pub fn to_url(&self, remote_base_url: &str) -> String {
        format!("{}/packages/{}/{}", remote_base_url, self.author, self.pkg)
    }

    /// Get the path to the dependency solver's cache folder for this package.
    ///
    /// This looks like `cache_home/elm_json_cache/author/package/`.
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

// Custom serialization for Pkg
impl Serialize for Pkg {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Pkg {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
