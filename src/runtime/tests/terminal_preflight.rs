use super::require_supported_terminal;

#[test]
fn force_bypasses_terminal_detection() {
    require_supported_terminal(true).expect("forced terminal check should pass");
}
