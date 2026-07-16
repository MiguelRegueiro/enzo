mod app;
mod drop_target;
mod font;
mod font_system;
mod input;
mod media;
mod overlay;
mod subtitle;
mod terminal;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("enzo: {error:#}");
        std::process::exit(1);
    }
}
