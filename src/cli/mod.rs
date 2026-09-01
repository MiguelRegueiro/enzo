mod argument_parsing;
mod command_execution;
mod media_drop_launcher;
mod terminal_input;
mod terminal_preflight;

pub(crate) use argument_parsing::{Action, HELP, Options, VERSION, parse_args};
pub(crate) use command_execution::run;
pub(crate) use terminal_input::{PlaybackCommand, PlaybackMouse, read_input_events};
