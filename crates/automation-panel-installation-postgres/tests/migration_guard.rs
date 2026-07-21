#[test]
fn strict_panel_migration_is_schema_pinned_and_staged() {
    let migration =
        include_str!("../../../migrations/202607220020_create_strict_panel_operation_journal.sql");
    let validation = include_str!(
        "../../../migrations/202607220021_validate_strict_panel_installation_constraints.sql"
    );
    for required in [
        "SET LOCAL search_path = pg_catalog, public",
        "ALTER TABLE public.ruleset_panel_installations",
        "CREATE TABLE public.strict_panel_operation_journal",
        "NOT VALID",
        ") IS TRUE",
    ] {
        assert!(
            migration.contains(required),
            "missing migration invariant: {required}"
        );
    }
    assert!(!migration.contains("VALIDATE CONSTRAINT"));
    assert!(!migration.contains("CREATE INDEX"));
    assert!(validation.contains("SET LOCAL search_path = pg_catalog, public"));
    assert_eq!(validation.matches("VALIDATE CONSTRAINT").count(), 5);
}

#[test]
fn adapter_sql_is_schema_qualified() {
    let sources = [
        include_str!("../src/store.rs"),
        include_str!("../src/strict_store.rs"),
    ];
    for source in sources {
        for forbidden in [
            "FROM ruleset_panel_installations",
            "INTO ruleset_panel_installations",
            "DELETE FROM ruleset_panel_installations",
            "FROM strict_panel_operation_journal",
            "INTO strict_panel_operation_journal",
            "DELETE FROM strict_panel_operation_journal",
        ] {
            assert!(
                !source.contains(forbidden),
                "unqualified SQL relation: {forbidden}"
            );
        }
    }
}
