//! `skterm-core`: Core terminal state machine, PTY abstraction, and Screen Grid buffer.

pub mod grid;
pub mod pty;
pub mod vt;

/// skterm-core version and basic info
pub fn core_info() -> &'static str {
    "skterm-core v0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_info() {
        assert_eq!(core_info(), "skterm-core v0.1.0");
    }
}
