mod config_definition;
mod config_loading;
mod config_saving;

#[allow(unused_imports, reason = "preserve the existing config facade")]
pub(crate) use {
    config_definition::{Config, DEFAULT_ACCENT_COLOR, MAX_VOLUME_MAX, MIN_VOLUME_MAX},
    config_loading::{config_dir, config_path},
};

pub(crate) use config_definition::parse_hex_color;
pub(crate) use config_saving::save_accent_color;
