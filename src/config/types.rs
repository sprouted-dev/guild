use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

/// A validated project name.
///
/// Must be non-empty and contain only lowercase alphanumeric characters, hyphens, and underscores.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectName(String);

impl ProjectName {
    pub fn new(s: &str) -> Result<Self, ParseError> {
        if s.is_empty() {
            return Err(ParseError::ProjectName {
                value: s.to_string(),
                reason: "must not be empty".to_string(),
            });
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(ParseError::ProjectName {
                value: s.to_string(),
                reason:
                    "must contain only lowercase alphanumeric characters, hyphens, and underscores"
                        .to_string(),
            });
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProjectName {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<ProjectName> for String {
    fn from(name: ProjectName) -> Self {
        name.0
    }
}

impl TryFrom<String> for ProjectName {
    type Error = ParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

/// A validated target name (e.g., "build", "test", "lint").
///
/// Must be non-empty and contain only lowercase alphanumeric characters, hyphens, and underscores.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TargetName(String);

impl TargetName {
    pub fn new(s: &str) -> Result<Self, ParseError> {
        if s.is_empty() {
            return Err(ParseError::TargetName {
                value: s.to_string(),
                reason: "must not be empty".to_string(),
            });
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(ParseError::TargetName {
                value: s.to_string(),
                reason:
                    "must contain only lowercase alphanumeric characters, hyphens, and underscores"
                        .to_string(),
            });
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for TargetName {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Display for TargetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<TargetName> for String {
    fn from(name: TargetName) -> Self {
        name.0
    }
}

impl TryFrom<String> for TargetName {
    type Error = ParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

/// A dependency reference in a target's `depends_on` list.
///
/// - `"build"` — depends on the local `build` target in the same project.
/// - `"^build"` — depends on the `build` target in all dependency projects.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DependsOn {
    /// Depends on a target in the same project.
    Local(TargetName),
    /// Depends on a target in upstream dependency projects (prefixed with `^`).
    Upstream(TargetName),
}

impl DependsOn {
    pub fn target_name(&self) -> &TargetName {
        match self {
            DependsOn::Local(name) | DependsOn::Upstream(name) => name,
        }
    }

    pub fn is_upstream(&self) -> bool {
        matches!(self, DependsOn::Upstream(_))
    }
}

impl FromStr for DependsOn {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ParseError::DependsOn {
                value: s.to_string(),
                reason: "must not be empty".to_string(),
            });
        }
        if let Some(rest) = s.strip_prefix('^') {
            let target = TargetName::new(rest).map_err(|_| ParseError::DependsOn {
                value: s.to_string(),
                reason: format!("invalid target name after '^': '{rest}'"),
            })?;
            Ok(DependsOn::Upstream(target))
        } else {
            let target = TargetName::new(s).map_err(|_| ParseError::DependsOn {
                value: s.to_string(),
                reason: format!("invalid target name: '{s}'"),
            })?;
            Ok(DependsOn::Local(target))
        }
    }
}

impl fmt::Display for DependsOn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DependsOn::Local(name) => write!(f, "{name}"),
            DependsOn::Upstream(name) => write!(f, "^{name}"),
        }
    }
}

impl Serialize for DependsOn {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DependsOn {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test helpers
    fn pname(s: &str) -> ProjectName {
        s.parse().unwrap()
    }
    fn tname(s: &str) -> TargetName {
        s.parse().unwrap()
    }
    fn dep(s: &str) -> DependsOn {
        s.parse().unwrap()
    }

    #[test]
    fn test_project_name_valid() {
        assert_eq!(pname("my-app").as_str(), "my-app");
        assert_eq!(pname("lib_utils").as_str(), "lib_utils");
        assert_eq!(pname("app123").as_str(), "app123");
    }

    #[test]
    fn test_project_name_empty_rejected() {
        assert!(ProjectName::new("").is_err());
    }

    #[test]
    fn test_project_name_uppercase_rejected() {
        assert!(ProjectName::new("MyApp").is_err());
    }

    #[test]
    fn test_project_name_display_roundtrip() {
        let name = pname("my-app");
        let roundtrip: ProjectName = name.to_string().parse().unwrap();
        assert_eq!(name, roundtrip);
    }

    #[test]
    fn test_target_name_valid() {
        assert_eq!(tname("build").as_str(), "build");
        assert_eq!(tname("test").as_str(), "test");
        assert_eq!(tname("type-check").as_str(), "type-check");
    }

    #[test]
    fn test_target_name_empty_rejected() {
        assert!(TargetName::new("").is_err());
    }

    #[test]
    fn test_target_name_display_roundtrip() {
        let name = tname("build");
        let roundtrip: TargetName = name.to_string().parse().unwrap();
        assert_eq!(name, roundtrip);
    }

    #[test]
    fn test_depends_on_local() {
        let d = dep("build");
        assert!(!d.is_upstream());
        assert_eq!(d.target_name().as_str(), "build");
        assert_eq!(d.to_string(), "build");
    }

    #[test]
    fn test_depends_on_upstream() {
        let d = dep("^build");
        assert!(d.is_upstream());
        assert_eq!(d.target_name().as_str(), "build");
        assert_eq!(d.to_string(), "^build");
    }

    #[test]
    fn test_depends_on_empty_rejected() {
        assert!(DependsOn::from_str("").is_err());
    }

    #[test]
    fn test_depends_on_display_roundtrip() {
        let local = dep("test");
        assert_eq!(local, local.to_string().parse().unwrap());

        let upstream = dep("^build");
        assert_eq!(upstream, upstream.to_string().parse().unwrap());
    }
}
