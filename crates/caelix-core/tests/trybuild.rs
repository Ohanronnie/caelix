#[test]
fn event_registration_compile_contracts() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/event_handler_missing_registerable.rs");
    t.compile_fail("tests/ui/event_handler_for_wrong_event.rs");
}
