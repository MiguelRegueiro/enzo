mod cli;
mod config;
mod font;
mod media;
mod overlay;
mod playback;
mod playlist;
mod resume;
mod runtime;
mod subtitle;
mod terminal;

fn main() {
    runtime::run();
}
