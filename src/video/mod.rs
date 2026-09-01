//! Video probing, decoding, frame storage, and playback timing.

mod video_decoder;
mod video_frame_decoder;
mod video_frame_store;
mod video_metadata;
mod video_timing;
mod video_worker;

pub(crate) use video_decoder::{FrameStatus, VideoDecoder};
pub(crate) use video_metadata::{VideoInfo, probe_video};

#[cfg(test)]
#[path = "tests/network_sources.rs"]
mod network_sources;
