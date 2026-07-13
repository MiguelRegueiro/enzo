mod app;
mod media;
mod terminal;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("rigoberto: {error:#}");
        std::process::exit(1);
    }
}
