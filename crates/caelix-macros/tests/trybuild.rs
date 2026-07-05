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
    t.compile_fail("tests/ui/controller_rejects_pattern_extractor.rs");
}
