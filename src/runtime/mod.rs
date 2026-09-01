mod command_execution;
mod error_reporting;
mod shutdown_signal;
mod terminal_preflight;

use std::env;

use anyhow::Result;

pub(crate) use shutdown_signal::shutdown_requested;

pub(crate) fn run() {
    if let Err(error) = run_result() {
        error_reporting::print_error(&error);
        std::process::exit(1);
    }
}

fn run_result() -> Result<()> {
    match crate::cli::parse_args(env::args_os().skip(1))? {
        crate::cli::Action::Run(options) => command_execution::execute(options),
        crate::cli::Action::Help => {
            print!("{}", crate::cli::HELP);
            Ok(())
        }
        crate::cli::Action::Version => {
            println!("{}", crate::cli::VERSION);
            Ok(())
        }
    }
}
