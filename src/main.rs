mod app;
mod font;
mod input;
mod media;
mod overlay;
mod terminal;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("rigoberto: {error:#}");
        std::process::exit(1);
    }
}
