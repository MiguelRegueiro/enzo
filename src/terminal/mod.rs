mod image_geometry;
mod kitty_graphics;
mod terminal_detection;
mod terminal_mode;

pub(crate) use image_geometry::{ImageArea, terminal_pixel_size};
pub(crate) use kitty_graphics::{
    KITTY_IMAGE_IDS, KITTY_PLACEMENT_ID, KittyFramePlacement, clear_screen_and_images,
    write_kitty_rgb_frame,
};
pub(crate) use terminal_detection::{enable_tmux_passthrough, inside_tmux, looks_like_kitty};
pub(crate) use terminal_mode::TerminalGuard;
