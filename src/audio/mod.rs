//! Audio playback and audio track probing.

mod audio_player;
mod audio_tracks;

pub(crate) use audio_player::AudioPlayer;
pub(crate) use audio_tracks::{AudioTrack, load_audio_tracks};
