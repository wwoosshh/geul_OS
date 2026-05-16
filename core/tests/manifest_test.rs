use geulos_core::{AppManifest, ManifestError, TypeUri};

#[test]
fn parse_minimal_manifest() {
    let toml = r#"
id = "memo"
permissions = []
ui_types = ["aios.std/Text@1"]
"#;
    let m = AppManifest::from_toml(toml).unwrap();
    assert_eq!(m.id, "memo");
    assert!(m.permissions.is_empty());
    assert_eq!(m.ui_types.len(), 1);
    assert_eq!(m.ui_types[0].as_str(), "aios.std/Text@1");
}

#[test]
fn parse_full_manifest() {
    let toml = r#"
id = "echo"
permissions = ["fs.user.docs", "clipboard.read"]
ui_types = ["aios.std/Container@1", "aios.std/Text@1", "aios.std/Button@1"]
"#;
    let m = AppManifest::from_toml(toml).unwrap();
    assert_eq!(m.id, "echo");
    assert_eq!(m.permissions.len(), 2);
    assert_eq!(m.ui_types.len(), 3);
}

#[test]
fn rejects_missing_id() {
    let toml = r#"
permissions = []
ui_types = []
"#;
    let err = AppManifest::from_toml(toml).unwrap_err();
    assert!(matches!(err, ManifestError::Toml(_)));
}

#[test]
fn rejects_invalid_ui_type_uri() {
    let toml = r#"
id = "bad"
permissions = []
ui_types = ["this is not a type uri"]
"#;
    let err = AppManifest::from_toml(toml).unwrap_err();
    assert!(matches!(err, ManifestError::BadTypeUri(_)));
}

#[test]
fn round_trip_via_to_toml() {
    let m = AppManifest {
        id: "test".to_string(),
        permissions: vec!["fs.user.docs".to_string()],
        ui_types: vec![TypeUri::parse("aios.std/Text@1").unwrap()],
    };
    let s = m.to_toml().unwrap();
    let back = AppManifest::from_toml(&s).unwrap();
    assert_eq!(m.id, back.id);
    assert_eq!(m.permissions, back.permissions);
    assert_eq!(m.ui_types.len(), back.ui_types.len());
}

#[test]
fn allows_known_type_uri() {
    let toml = r#"
id = "x"
permissions = []
ui_types = ["aios.std/Button@1", "x/Custom@2"]
"#;
    let m = AppManifest::from_toml(toml).unwrap();
    assert_eq!(m.ui_types.len(), 2);
}
