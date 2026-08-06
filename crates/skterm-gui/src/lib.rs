//! `skterm-gui`: Windowing, tabs, split panes (multiplexing), input handling, and clipboard.

pub mod app_window;
pub mod config;
pub mod input;
pub mod layout;

/// skterm-gui version and basic info
pub fn gui_info() -> &'static str {
    "skterm-gui v0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gui_info() {
        assert_eq!(gui_info(), "skterm-gui v0.1.0");
    }
}
