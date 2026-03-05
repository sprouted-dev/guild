use proptest::prelude::*;

use guild_cli::{DependsOn, ProjectName, TargetName};

proptest! {
    #[test]
    fn test_project_name_roundtrip(s in "[a-z][a-z0-9_-]{0,30}") {
        let name: ProjectName = s.parse().unwrap();
        let roundtrip: ProjectName = name.to_string().parse().unwrap();
        prop_assert_eq!(name, roundtrip);
    }

    #[test]
    fn test_target_name_roundtrip(s in "[a-z][a-z0-9_-]{0,30}") {
        let name: TargetName = s.parse().unwrap();
        let roundtrip: TargetName = name.to_string().parse().unwrap();
        prop_assert_eq!(name, roundtrip);
    }

    #[test]
    fn test_depends_on_local_roundtrip(s in "[a-z][a-z0-9_-]{0,30}") {
        let dep: DependsOn = s.parse().unwrap();
        let roundtrip: DependsOn = dep.to_string().parse().unwrap();
        prop_assert_eq!(dep, roundtrip);
    }

    #[test]
    fn test_depends_on_upstream_roundtrip(s in "[a-z][a-z0-9_-]{0,30}") {
        let input = format!("^{s}");
        let dep: DependsOn = input.parse().unwrap();
        let roundtrip: DependsOn = dep.to_string().parse().unwrap();
        prop_assert_eq!(dep, roundtrip);
    }

    #[test]
    fn test_project_name_rejects_empty(s in "\\s*") {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            prop_assert!(trimmed.parse::<ProjectName>().is_err());
        }
    }

    #[test]
    fn test_project_name_rejects_uppercase(s in "[A-Z][a-zA-Z0-9]{0,10}") {
        prop_assert!(s.parse::<ProjectName>().is_err());
    }
}
