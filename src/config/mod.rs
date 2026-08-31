mod config_definition;
mod config_loading;

#[allow(unused_imports, reason = "preserve the existing config facade")]
pub(crate) use {
    config_definition::{Config, DEFAULT_ACCENT_COLOR, MAX_VOLUME_MAX, MIN_VOLUME_MAX},
    config_loading::config_dir,
};
