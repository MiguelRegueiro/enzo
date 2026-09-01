//! Raw declarations for Enzo's C media ABI.
//!
//! Keep this module synchronized with `csrc/media.h`. Higher-level subsystems
//! should wrap these declarations in focused audio, video, or subtitle APIs.

use std::ffi::{c_char, c_double, c_int, c_uchar};

pub(crate) const INFO_TEXT_LEN: usize = 64;
pub(crate) const TRACK_TEXT_LEN: usize = 128;
pub(crate) const HDR_PQ: c_int = 1;
pub(crate) const HDR_HLG: c_int = 2;
pub(crate) const SUBTITLE_TEXT: c_int = 1;
pub(crate) const SUBTITLE_ASS: c_int = 2;
pub(crate) const SUBTITLE_BITMAP: c_int = 3;
pub(crate) const SUBTITLE_PALETTE_BYTES: usize = 256 * 4;

#[repr(C)]
pub(crate) struct EnzoVideoInfo {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) fps: c_double,
    pub(crate) duration: c_double,
    pub(crate) has_audio: c_int,
    pub(crate) seekable: c_int,
    pub(crate) codec: [c_char; INFO_TEXT_LEN],
    pub(crate) profile: [c_char; INFO_TEXT_LEN],
    pub(crate) container: [c_char; INFO_TEXT_LEN],
    pub(crate) hdr: c_int,
}

#[repr(C)]
pub(crate) struct EnzoAudioTrackInfo {
    pub(crate) stream_index: c_int,
    pub(crate) channels: c_int,
    pub(crate) sample_rate: c_int,
    pub(crate) is_default: c_int,
    pub(crate) codec: [c_char; TRACK_TEXT_LEN],
    pub(crate) channel_layout: [c_char; TRACK_TEXT_LEN],
    pub(crate) language: [c_char; TRACK_TEXT_LEN],
    pub(crate) title: [c_char; TRACK_TEXT_LEN],
}

#[repr(C)]
pub(crate) struct EnzoSubtitleStreamInfo {
    pub(crate) subtitle_index: c_int,
    pub(crate) is_default: c_int,
    pub(crate) is_forced: c_int,
    pub(crate) codec: [c_char; TRACK_TEXT_LEN],
    pub(crate) language: [c_char; TRACK_TEXT_LEN],
    pub(crate) title: [c_char; TRACK_TEXT_LEN],
}

#[repr(C)]
pub(crate) struct EnzoDecodedSubtitleCue {
    pub(crate) start_micros: i64,
    pub(crate) end_micros: i64,
    pub(crate) text_kind: c_int,
    pub(crate) text: *mut c_char,
    pub(crate) bitmap_x: u32,
    pub(crate) bitmap_y: u32,
    pub(crate) bitmap_width: u32,
    pub(crate) bitmap_height: u32,
    pub(crate) bitmap_indices: *mut c_uchar,
    pub(crate) palette_rgba: [u8; SUBTITLE_PALETTE_BYTES],
}

#[repr(C)]
pub(crate) struct EnzoDecodedSubtitleTrack {
    pub(crate) cues: *mut EnzoDecodedSubtitleCue,
    pub(crate) count: usize,
    pub(crate) capacity: usize,
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
}

#[repr(C)]
pub(crate) struct EnzoVideoDecoderOpaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    pub(crate) fn enzo_file_fingerprint(
        path: *const c_char,
        len: u64,
        chunk_len: u64,
        out: *mut c_uchar,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(crate) fn enzo_probe_video(
        path: *const c_char,
        out: *mut EnzoVideoInfo,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(crate) fn enzo_probe_audio_tracks(
        path: *const c_char,
        tracks_out: *mut *mut EnzoAudioTrackInfo,
        count_out: *mut usize,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(crate) fn enzo_audio_tracks_free(tracks: *mut EnzoAudioTrackInfo);

    pub(crate) fn enzo_probe_subtitle_streams(
        path: *const c_char,
        streams_out: *mut *mut EnzoSubtitleStreamInfo,
        count_out: *mut usize,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(crate) fn enzo_subtitle_streams_free(streams: *mut EnzoSubtitleStreamInfo);

    pub(crate) fn enzo_decode_subtitle_stream(
        path: *const c_char,
        subtitle_index: c_int,
        track_out: *mut EnzoDecodedSubtitleTrack,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(crate) fn enzo_decoded_subtitle_track_free(track: *mut EnzoDecodedSubtitleTrack);

    pub(crate) fn enzo_video_decoder_open(
        path: *const c_char,
        out_width: c_int,
        out_height: c_int,
        fps: c_double,
        out: *mut *mut EnzoVideoDecoderOpaque,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(crate) fn enzo_video_decoder_next(
        decoder: *mut EnzoVideoDecoderOpaque,
        rgb_out: *mut c_uchar,
        rgb_len: usize,
        pts_out: *mut c_double,
        drop_before_pts: c_double,
        stop_flag: *const c_int,
        seek_generation: *const c_int,
        expected_seek_generation: c_int,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(crate) fn enzo_video_decoder_seek(
        decoder: *mut EnzoVideoDecoderOpaque,
        seconds: c_double,
        exact: c_int,
        stop_flag: *const c_int,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(crate) fn enzo_video_decoder_close(decoder: *mut EnzoVideoDecoderOpaque);

    pub(crate) fn enzo_play_audio(
        path: *const c_char,
        audio_stream_index: c_int,
        stop_flag: *const c_int,
        pause_flag: *const c_int,
        mute_flag: *const c_int,
        volume_percent: *const c_int,
        seek_generation: *const c_int,
        seek_micros: *const i64,
        released_seek_generation: *const c_int,
        applied_seek_generation: *mut c_int,
        buffered_seek_generation: *mut c_int,
        playback_micros: *mut i64,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    #[cfg(test)]
    pub(crate) fn enzo_audio_seek_trim_samples(
        frame_timestamp: i64,
        timestamp_origin: i64,
        time_base_num: c_int,
        time_base_den: c_int,
        frame_samples: c_int,
        source_rate: c_int,
        target_micros: i64,
        delayed_output_samples: c_int,
        converted_samples: c_int,
    ) -> c_int;

    #[cfg(test)]
    pub(crate) fn enzo_audio_seek_leading_silence_samples(
        frame_timestamp: i64,
        timestamp_origin: i64,
        time_base_num: c_int,
        time_base_den: c_int,
        target_micros: i64,
    ) -> c_int;
}
