mod app;
mod font;
mod font_system;
mod media;
mod overlay;
mod resume;
mod shutdown;
mod subtitle;
mod subtitle_language;
mod terminal;
mod text_layout;

fn main() {
    if let Err(error) = app::run() {
        print_error(&error);
        std::process::exit(1);
    }
}

fn print_error(error: &anyhow::Error) {
    let mut chain = error.chain();
    let Some(message) = chain.next() else {
        eprintln!("\x1b[31munknown error\x1b[0m");
        return;
    };

    let message = message.to_string();
    let mut lines = message.lines();
    if let Some(headline) = lines.next() {
        eprintln!("\x1b[31m{headline}\x1b[0m");
    }
    for line in lines {
        eprintln!("{line}");
    }
    for cause in chain {
        eprintln!("  cause: {cause}");
    }
}
