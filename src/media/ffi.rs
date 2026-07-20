//! Raw declarations for Enzo's C media ABI.
//!
//! Keep this module synchronized with `csrc/media.h`. Code outside the parent
//! `media` module should use safe Rust wrappers rather than calling these
//! functions directly.

use std::ffi::{c_char, c_double, c_int, c_uchar};

pub(super) const INFO_TEXT_LEN: usize = 64;
pub(super) const TRACK_TEXT_LEN: usize = 128;
pub(super) const HDR_PQ: c_int = 1;
pub(super) const HDR_HLG: c_int = 2;
pub(super) const SUBTITLE_TEXT: c_int = 1;
pub(super) const SUBTITLE_ASS: c_int = 2;
pub(super) const SUBTITLE_BITMAP: c_int = 3;
pub(super) const SUBTITLE_PALETTE_BYTES: usize = 256 * 4;

#[repr(C)]
pub(super) struct EnzoVideoInfo {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) fps: c_double,
    pub(super) duration: c_double,
    pub(super) has_audio: c_int,
    pub(super) seekable: c_int,
    pub(super) codec: [c_char; INFO_TEXT_LEN],
    pub(super) profile: [c_char; INFO_TEXT_LEN],
    pub(super) container: [c_char; INFO_TEXT_LEN],
    pub(super) hdr: c_int,
}

#[repr(C)]
pub(super) struct EnzoAudioTrackInfo {
    pub(super) stream_index: c_int,
    pub(super) channels: c_int,
    pub(super) sample_rate: c_int,
    pub(super) is_default: c_int,
    pub(super) codec: [c_char; TRACK_TEXT_LEN],
    pub(super) channel_layout: [c_char; TRACK_TEXT_LEN],
    pub(super) language: [c_char; TRACK_TEXT_LEN],
    pub(super) title: [c_char; TRACK_TEXT_LEN],
}

#[repr(C)]
pub(super) struct EnzoSubtitleStreamInfo {
    pub(super) subtitle_index: c_int,
    pub(super) is_default: c_int,
    pub(super) is_forced: c_int,
    pub(super) codec: [c_char; TRACK_TEXT_LEN],
    pub(super) language: [c_char; TRACK_TEXT_LEN],
    pub(super) title: [c_char; TRACK_TEXT_LEN],
}

#[repr(C)]
pub(super) struct EnzoDecodedSubtitleCue {
    pub(super) start_micros: i64,
    pub(super) end_micros: i64,
    pub(super) text_kind: c_int,
    pub(super) text: *mut c_char,
    pub(super) bitmap_x: u32,
    pub(super) bitmap_y: u32,
    pub(super) bitmap_width: u32,
    pub(super) bitmap_height: u32,
    pub(super) bitmap_indices: *mut c_uchar,
    pub(super) palette_rgba: [u8; SUBTITLE_PALETTE_BYTES],
}

#[repr(C)]
pub(super) struct EnzoDecodedSubtitleTrack {
    pub(super) cues: *mut EnzoDecodedSubtitleCue,
    pub(super) count: usize,
    pub(super) capacity: usize,
    pub(super) canvas_width: u32,
    pub(super) canvas_height: u32,
}

#[repr(C)]
pub(super) struct EnzoVideoDecoderOpaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    pub(super) fn enzo_file_fingerprint(
        path: *const c_char,
        len: u64,
        chunk_len: u64,
        out: *mut c_uchar,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(super) fn enzo_probe_video(
        path: *const c_char,
        out: *mut EnzoVideoInfo,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(super) fn enzo_probe_audio_tracks(
        path: *const c_char,
        tracks_out: *mut *mut EnzoAudioTrackInfo,
        count_out: *mut usize,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(super) fn enzo_audio_tracks_free(tracks: *mut EnzoAudioTrackInfo);

    pub(super) fn enzo_probe_subtitle_streams(
        path: *const c_char,
        streams_out: *mut *mut EnzoSubtitleStreamInfo,
        count_out: *mut usize,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(super) fn enzo_subtitle_streams_free(streams: *mut EnzoSubtitleStreamInfo);

    pub(super) fn enzo_decode_subtitle_stream(
        path: *const c_char,
        subtitle_index: c_int,
        track_out: *mut EnzoDecodedSubtitleTrack,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(super) fn enzo_decoded_subtitle_track_free(track: *mut EnzoDecodedSubtitleTrack);

    pub(super) fn enzo_video_decoder_open(
        path: *const c_char,
        out_width: c_int,
        out_height: c_int,
        fps: c_double,
        out: *mut *mut EnzoVideoDecoderOpaque,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(super) fn enzo_video_decoder_next(
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

    pub(super) fn enzo_video_decoder_seek(
        decoder: *mut EnzoVideoDecoderOpaque,
        seconds: c_double,
        exact: c_int,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    pub(super) fn enzo_video_decoder_close(decoder: *mut EnzoVideoDecoderOpaque);

    pub(super) fn enzo_play_audio(
        path: *const c_char,
        audio_stream_index: c_int,
        stop_flag: *const c_int,
        pause_flag: *const c_int,
        mute_flag: *const c_int,
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
    pub(super) fn enzo_audio_seek_trim_samples(
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
    pub(super) fn enzo_audio_seek_leading_silence_samples(
        frame_timestamp: i64,
        timestamp_origin: i64,
        time_base_num: c_int,
        time_base_den: c_int,
        target_micros: i64,
    ) -> c_int;
}
