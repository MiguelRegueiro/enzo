//! User-facing media source parsing and validation.

mod source_parsing;
mod source_resolution;

pub(crate) use source_parsing::media_candidates_from_text;
pub(crate) use source_resolution::{
    is_remote_url_text, media_path_from_argument, media_path_from_drop_text, validate_subtitle_path,
};
