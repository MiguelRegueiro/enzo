use super::*;

#[test]
fn error_buffer_uses_fallback_when_empty() {
    let error = ErrorBuffer::new();

    assert_eq!(error.message("fallback"), "fallback");
}
