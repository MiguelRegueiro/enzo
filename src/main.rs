mod app;
mod font;
mod font_system;
mod media;
mod overlay;
mod resume;
mod shutdown;
mod subtitle;
mod terminal;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("enzo: {error:#}");
        std::process::exit(1);
    }
}
