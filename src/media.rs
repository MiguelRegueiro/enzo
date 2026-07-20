//! Safe Rust interface to Enzo's native media backend.
//!
//! Metadata inspection, playback, subtitle decoding, and fingerprinting are
//! exposed through safe types while the raw C ABI remains private here.

mod audio;
mod ffi;
mod fingerprint;
mod native;
mod probe;
mod subtitle;
mod video;

pub(crate) use audio::AudioPlayer;
pub(crate) use fingerprint::file_fingerprint_digest;
pub(crate) use probe::{
    AudioTrack, SubtitleStreamInfo, VideoInfo, load_audio_tracks, load_subtitle_streams,
    probe_video,
};
pub(crate) use subtitle::{
    DecodedSubtitleBitmap, DecodedSubtitleCue, DecodedSubtitleTextKind, decode_subtitle_stream,
};
pub(crate) use video::{FrameStatus, VideoDecoder};
