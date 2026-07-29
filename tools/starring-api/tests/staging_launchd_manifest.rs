const STAGING_API_PLIST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/macos/local.starring.api.staging.plist"
));

fn environment_value(name: &str) -> &str {
    let marker = format!("<key>{name}</key>\n        <string>");
    let (_, remaining) = STAGING_API_PLIST.split_once(&marker).unwrap();
    remaining.split_once("</string>").unwrap().0
}

#[test]
fn staging_launchd_manifest_pins_optional_authoring_references_without_raw_secrets() {
    assert_eq!(
        environment_value("STARRING_API_AUTHORING_SESSION_WRITER_DATABASE_SECRET_REFERENCE"),
        "keychain:starring-api.staging:database.authoring-session-writer"
    );
    assert_eq!(
        environment_value("STARRING_API_AUTHORING_WORKER_URL"),
        "http://127.0.0.1:18181"
    );
    assert_eq!(
        environment_value("STARRING_API_AUTHORING_WORKER_TOKEN_SECRET_REFERENCE"),
        "keychain:com.starring.llm-api-key:llm-api"
    );
    assert!(!STAGING_API_PLIST.contains("<key>STARRING_API_AUTHORING_WORKER_TOKEN</key>"));
    assert!(!STAGING_API_PLIST.contains("<key>STARRING_CODEX_WORKER_TOKEN</key>"));
}
