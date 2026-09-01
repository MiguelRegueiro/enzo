mod audio;
mod cli;
mod config;
mod decoder_backend;
mod font;
mod media_source;
mod overlay;
mod playback;
mod playlist;
mod resume;
mod runtime;
mod subtitle;
mod terminal;
mod video;

fn main() {
    runtime::run();
}
