//! Module dealing with project configuration in the elm.json file.

use crate::constraint::Constraint;
use pubgrub::range::Range;
use pubgrub::version::SemanticVersion as SemVer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap as Map;

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
    pub direct: Map<String, SemVer>,
    pub indirect: Map<String, SemVer>,
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
    pub dependencies: Map<String, Constraint>,
    pub test_dependencies: Map<String, Constraint>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExposedModules {
    NoCategory(Vec<String>),
    WithCategories(Map<String, Vec<String>>),
}

impl PackageConfig {
    pub fn dependencies_iter(&self) -> impl Iterator<Item = (&String, &Range<SemVer>)> {
        self.dependencies
            .iter()
            .map(|(p, constraint)| (p, &constraint.0))
    }
}
