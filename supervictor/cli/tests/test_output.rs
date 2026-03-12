use qs::output;

#[test]
fn test_verbose_flag() {
    output::reset_state();
    assert!(!output::is_verbose());
    output::set_verbose(true);
    assert!(output::is_verbose());
    output::set_verbose(false);
    assert!(!output::is_verbose());
}
