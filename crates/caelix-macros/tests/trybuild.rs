#[test]
fn injectable_macro_compile_contracts() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/injectable_named_struct.rs");
    t.pass("tests/ui/injectable_unit_struct.rs");
    t.compile_fail("tests/ui/injectable_rejects_non_arc_field.rs");
    t.compile_fail("tests/ui/injectable_rejects_tuple_struct.rs");
}

#[test]
fn controller_macro_compile_contracts() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/controller_with_extractors.rs");
    t.pass("tests/ui/controller_namespaced_state_names.rs");
    t.compile_fail("tests/ui/controller_rejects_pattern_extractor.rs");
    #[cfg(feature = "uploads")]
    t.compile_fail("tests/ui/controller_rejects_missing_upload_validator.rs");
    #[cfg(feature = "uploads")]
    t.compile_fail("tests/ui/controller_rejects_invalid_upload_validator.rs");
}

#[test]
#[cfg(feature = "config")]
fn config_macro_compile_contracts() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/config_named_struct.rs");
    t.compile_fail("tests/ui/config_rejects_duplicate_env.rs");
    t.compile_fail("tests/ui/config_rejects_generics.rs");
    t.compile_fail("tests/ui/config_rejects_invalid_attribute.rs");
    t.compile_fail("tests/ui/config_rejects_missing_mapping.rs");
    t.compile_fail("tests/ui/config_rejects_missing_deserialize.rs");
    t.compile_fail("tests/ui/config_rejects_missing_validate.rs");
    t.compile_fail("tests/ui/config_rejects_tuple_struct.rs");
}
