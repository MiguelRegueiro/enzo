//! Safe Rust interface to Enzo's native media backend.
//!
//! Metadata inspection, playback, subtitle decoding, and fingerprinting are
//! exposed through safe types while the raw C ABI remains private here.

mod audio_player;
mod audio_tracks;
mod ffi_support;
mod file_fingerprint;
mod media_ffi;
mod metadata_display;
mod source_parsing;
mod source_resolution;
mod subtitle_decoder;
mod subtitle_streams;
mod video_decoder;
mod video_frame_decoder;
mod video_frame_store;
mod video_metadata;
mod video_timing;
mod video_worker;

pub(crate) use audio_player::AudioPlayer;
pub(crate) use audio_tracks::{AudioTrack, load_audio_tracks};
pub(crate) use file_fingerprint::file_fingerprint_digest;
pub(crate) use source_parsing::media_candidates_from_text;
pub(crate) use source_resolution::{
    is_remote_url_text, media_path_from_argument, media_path_from_drop_text, validate_subtitle_path,
};
pub(crate) use subtitle_decoder::{
    DecodedSubtitleBitmap, DecodedSubtitleCue, DecodedSubtitleTextKind, decode_subtitle_stream,
};
pub(crate) use subtitle_streams::{SubtitleStreamInfo, load_subtitle_streams};
pub(crate) use video_decoder::{FrameStatus, VideoDecoder};
pub(crate) use video_metadata::{VideoInfo, probe_video};

#[cfg(test)]
#[path = "tests/network_sources.rs"]
mod network_sources;
