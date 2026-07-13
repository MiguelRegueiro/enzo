mod env;
mod kitty_graphics;
mod layout;
mod session;

pub(crate) use env::{enable_tmux_passthrough, inside_tmux, looks_like_kitty};
pub(crate) use kitty_graphics::{
    KITTY_IMAGE_IDS, KITTY_PLACEMENT_ID, KittyFramePlacement, clear_screen_and_images,
    write_kitty_rgb_frame,
};
pub(crate) use layout::{ImageArea, image_area_for_terminal, terminal_pixel_size};
pub(crate) use session::TerminalGuard;
