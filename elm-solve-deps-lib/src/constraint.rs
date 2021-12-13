// SPDX-License-Identifier: MPL-2.0

//! Module helping with serialization and deserialization of version constraints.

use pubgrub::range::Range;
use pubgrub::version::{SemanticVersion as SemVer, VersionParseError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;
use thiserror::Error;

/// A constraint is a simple newtype for ranges of versions defined in the pubgrub crate.
#[derive(Debug, Clone)]
pub struct Constraint(pub Range<SemVer>);

/// Error creating [Constraint] from [String].
#[derive(Error, Debug, PartialEq)]
pub enum ConstraintParseError {
    /// Constraint must have the shape "v1 <= v < v2".
    #[error(
        "Invalid format \"{full_constraint}\": constraint must have the shape \"v1 <= v < v2\""
    )]
    InvalidFormat {
        /// Constraint that was being parsed.
        full_constraint: String,
    },
    /// Allowed separators are "<=" and "<".
    #[error("Invalid separators \"{full_constraint}\": the only separators allowed are \"<=\" and \"<\"")]
    InvalidSeparator {
        /// Constraint that was being parsed.
        full_constraint: String,
    },
    /// Invalid version.
    #[error("Invalid version in constraint")]
    InvalidVersion(VersionParseError),
}

impl FromStr for Constraint {
    type Err = ConstraintParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split_whitespace().collect();
        match *parts.as_slice() {
            [low, sep1, _, sep2, high] => {
                let v1: SemVer = FromStr::from_str(low).map_err(Self::Err::InvalidVersion)?;
                let v2: SemVer = FromStr::from_str(high).map_err(Self::Err::InvalidVersion)?;
                if sep1 != "<=" && sep1 != "<" {
                    return Err(Self::Err::InvalidSeparator {
                        full_constraint: s.to_string(),
                    });
                }
                if sep2 != "<=" && sep2 != "<" {
                    return Err(Self::Err::InvalidSeparator {
                        full_constraint: s.to_string(),
                    });
                }
                let range1 = if sep1 == "<=" {
                    Range::higher_than(v1)
                } else {
                    Range::higher_than(v1.bump_patch())
                };
                let range2 = if sep2 == "<" {
                    Range::strictly_lower_than(v2)
                } else {
                    Range::strictly_lower_than(v2.bump_patch())
                };
                let range = range1.intersection(&range2);
                Ok(Self(range))
            }
            _ => Err(Self::Err::InvalidFormat {
                full_constraint: s.to_string(),
            }),
        }
    }
}

impl Serialize for Constraint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.to_string().as_str())
    }
}

impl<'de> Deserialize<'de> for Constraint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(serde::de::Error::custom)
    }
}
