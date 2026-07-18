#include "audio_output.h"
#include "internal.h"

#include <libavcodec/avcodec.h>
#include <libavutil/channel_layout.h>
#include <libavutil/imgutils.h>
#include <libavutil/mem.h>
#include <libavutil/mathematics.h>
#include <libavutil/time.h>
#include <libswresample/swresample.h>
#include <stdint.h>
#include <string.h>

typedef struct EnzoAudioConverter {
    SwrContext *swr;
    AVChannelLayout src_layout;
    enum AVSampleFormat src_format;
    int src_rate;
    uint8_t *out_buffer;
    int out_capacity;
    int configured;
} EnzoAudioConverter;

typedef struct EnzoAudioSeekState {
    int active;
    int64_t target_micros;
} EnzoAudioSeekState;

static int open_audio_decoder(
    const char *path,
    int requested_stream_index,
    AVFormatContext **format_out,
    AVCodecContext **codec_out,
    int *stream_index_out,
    char *err,
    size_t err_len
) {
    AVFormatContext *format = NULL;
    int ret = avformat_open_input(&format, path, NULL, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to open audio input", ret);
        return -1;
    }

    ret = avformat_find_stream_info(format, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to read audio stream info", ret);
        avformat_close_input(&format);
        return -1;
    }

    int stream_index = requested_stream_index;
    if (stream_index >= 0) {
        if ((unsigned int)stream_index >= format->nb_streams ||
            format->streams[stream_index]->codecpar->codec_type != AVMEDIA_TYPE_AUDIO) {
            enzo_set_error(err, err_len, "selected audio stream is not available");
            avformat_close_input(&format);
            return -1;
        }
    } else {
        stream_index = av_find_best_stream(format, AVMEDIA_TYPE_AUDIO, -1, -1, NULL, 0);
    }
    if (stream_index < 0) {
        avformat_close_input(&format);
        return 0;
    }

    AVStream *stream = format->streams[stream_index];
    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (codec == NULL) {
        enzo_set_error(err, err_len, "failed to find audio decoder");
        avformat_close_input(&format);
        return -1;
    }

    AVCodecContext *codec_context = avcodec_alloc_context3(codec);
    if (codec_context == NULL) {
        enzo_set_error(err, err_len, "failed to allocate audio codec context");
        avformat_close_input(&format);
        return -1;
    }

    ret = avcodec_parameters_to_context(codec_context, stream->codecpar);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to copy audio codec parameters", ret);
        avcodec_free_context(&codec_context);
        avformat_close_input(&format);
        return -1;
    }

    ret = avcodec_open2(codec_context, codec, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to open audio decoder", ret);
        avcodec_free_context(&codec_context);
        avformat_close_input(&format);
        return -1;
    }

    *format_out = format;
    *codec_out = codec_context;
    *stream_index_out = stream_index;
    return 1;
}

static void audio_converter_init(EnzoAudioConverter *converter) {
    memset(converter, 0, sizeof(*converter));
    converter->src_format = AV_SAMPLE_FMT_NONE;
}

static void audio_converter_reset(EnzoAudioConverter *converter) {
    if (converter == NULL) {
        return;
    }
    swr_free(&converter->swr);
    if (converter->src_layout.nb_channels > 0) {
        av_channel_layout_uninit(&converter->src_layout);
    }
    memset(&converter->src_layout, 0, sizeof(converter->src_layout));
    converter->src_format = AV_SAMPLE_FMT_NONE;
    converter->src_rate = 0;
    converter->configured = 0;
}

static void audio_converter_close(EnzoAudioConverter *converter) {
    if (converter == NULL) {
        return;
    }
    audio_converter_reset(converter);
    av_freep(&converter->out_buffer);
    converter->out_capacity = 0;
}

static int copy_audio_frame_layout(
    const AVCodecContext *codec,
    const AVFrame *frame,
    AVChannelLayout *layout,
    char *err,
    size_t err_len
) {
    memset(layout, 0, sizeof(*layout));
    if (frame != NULL && frame->ch_layout.nb_channels > 0) {
        int ret = av_channel_layout_copy(layout, &frame->ch_layout);
        if (ret < 0) {
            enzo_set_ffmpeg_error(err, err_len, "failed to copy audio frame layout", ret);
            return -1;
        }
        return 0;
    }
    if (codec != NULL && codec->ch_layout.nb_channels > 0) {
        int ret = av_channel_layout_copy(layout, &codec->ch_layout);
        if (ret < 0) {
            enzo_set_ffmpeg_error(err, err_len, "failed to copy audio codec layout", ret);
            return -1;
        }
        return 0;
    }

    av_channel_layout_default(layout, ENZO_AUDIO_OUTPUT_CHANNELS);
    return 0;
}

static int audio_converter_configure(
    EnzoAudioConverter *converter,
    const AVCodecContext *codec,
    const AVFrame *frame,
    char *err,
    size_t err_len
) {
    int src_rate = frame->sample_rate > 0 ? frame->sample_rate : codec->sample_rate;
    if (src_rate <= 0) {
        enzo_set_error(err, err_len, "invalid audio sample rate");
        return -1;
    }

    enum AVSampleFormat src_format = (enum AVSampleFormat)frame->format;
    if (src_format == AV_SAMPLE_FMT_NONE) {
        src_format = codec->sample_fmt;
    }
    if (src_format == AV_SAMPLE_FMT_NONE) {
        enzo_set_error(err, err_len, "invalid audio sample format");
        return -1;
    }

    AVChannelLayout src_layout;
    if (copy_audio_frame_layout(codec, frame, &src_layout, err, err_len) < 0) {
        return -1;
    }

    if (converter->configured && converter->src_rate == src_rate &&
        converter->src_format == src_format &&
        av_channel_layout_compare(&converter->src_layout, &src_layout) == 0) {
        av_channel_layout_uninit(&src_layout);
        return 0;
    }

    AVChannelLayout dst_layout;
    av_channel_layout_default(&dst_layout, ENZO_AUDIO_OUTPUT_CHANNELS);

    SwrContext *swr = NULL;
    int ret = swr_alloc_set_opts2(
        &swr,
        &dst_layout,
        AV_SAMPLE_FMT_S16,
        ENZO_AUDIO_OUTPUT_RATE,
        &src_layout,
        src_format,
        src_rate,
        0,
        NULL
    );
    av_channel_layout_uninit(&dst_layout);

    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to allocate audio resampler", ret);
        av_channel_layout_uninit(&src_layout);
        swr_free(&swr);
        return -1;
    }
    if (swr == NULL) {
        enzo_set_error(err, err_len, "failed to allocate audio resampler");
        av_channel_layout_uninit(&src_layout);
        return -1;
    }

    ret = swr_init(swr);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to initialize audio resampler", ret);
        av_channel_layout_uninit(&src_layout);
        swr_free(&swr);
        return -1;
    }

    audio_converter_reset(converter);
    converter->swr = swr;
    converter->src_layout = src_layout;
    converter->src_format = src_format;
    converter->src_rate = src_rate;
    converter->configured = 1;
    return 0;
}

static int seek_audio_decoder(
    AVFormatContext *format,
    AVCodecContext *codec,
    int stream_index,
    int64_t micros,
    char *err,
    size_t err_len
) {
    AVStream *stream = format->streams[stream_index];
    AVRational micros_base = {1, 1000000};
    int64_t timestamp =
        av_rescale_q(micros, micros_base, stream->time_base) +
        enzo_stream_timestamp_origin(format, stream);
    int ret = av_seek_frame(format, stream_index, timestamp, AVSEEK_FLAG_BACKWARD);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to seek audio", ret);
        return -1;
    }

    avcodec_flush_buffers(codec);
    return 0;
}

static int sync_audio_seek(
    EnzoPulseOutput *pulse,
    AVFormatContext *format,
    AVCodecContext *codec,
    EnzoAudioConverter *converter,
    int stream_index,
    AVPacket *packet,
    AVFrame *frame,
    const int *seek_generation,
    const int64_t *seek_micros,
    int *seen_seek_generation,
    int *applied_seek_generation,
    int *corked,
    int *flushing,
    EnzoAudioSeekState *seek_state,
    EnzoAudioClock *clock,
    int64_t *playback_micros,
    char *err,
    size_t err_len
) {
    int64_t micros = 0;
    if (!enzo_take_seek_request(seek_generation, seek_micros, seen_seek_generation, &micros)) {
        return 0;
    }

    if (enzo_pulse_output_prepare_seek(pulse, corked, err, err_len) < 0) {
        return -1;
    }

    if (packet != NULL) {
        av_packet_unref(packet);
    }
    if (frame != NULL) {
        av_frame_unref(frame);
    }
    if (seek_audio_decoder(format, codec, stream_index, micros, err, err_len) < 0) {
        return -1;
    }
    audio_converter_reset(converter);
    *flushing = 0;
    seek_state->active = 1;
    seek_state->target_micros = micros;
    enzo_audio_clock_reset(
        clock,
        *seen_seek_generation,
        micros,
        playback_micros
    );
    enzo_atomic_store_generation(applied_seek_generation, *seen_seek_generation);
    return 1;
}

int enzo_audio_seek_trim_samples(
    int64_t frame_timestamp,
    int64_t timestamp_origin,
    int time_base_num,
    int time_base_den,
    int frame_samples,
    int source_rate,
    int64_t target_micros,
    int delayed_output_samples,
    int converted_samples
) {
    if (time_base_num <= 0 || time_base_den <= 0 || frame_samples < 0 ||
        source_rate <= 0 || delayed_output_samples < 0 || converted_samples < 0) {
        return -1;
    }

    AVRational time_base = {time_base_num, time_base_den};
    AVRational micros_base = {1, 1000000};
    int64_t frame_start_micros = av_rescale_q(
        frame_timestamp - timestamp_origin,
        time_base,
        micros_base
    );
    int64_t frame_duration_micros = av_rescale_q(
        frame_samples,
        (AVRational){1, source_rate},
        micros_base
    );
    if (frame_start_micros + frame_duration_micros <= target_micros) {
        return -1;
    }

    int64_t skip_samples = delayed_output_samples;
    if (frame_start_micros < target_micros) {
        skip_samples += av_rescale_rnd(
            target_micros - frame_start_micros,
            ENZO_AUDIO_OUTPUT_RATE,
            1000000,
            AV_ROUND_UP
        );
    }
    return skip_samples >= converted_samples ? -1 : (int)skip_samples;
}

int enzo_audio_seek_leading_silence_samples(
    int64_t frame_timestamp,
    int64_t timestamp_origin,
    int time_base_num,
    int time_base_den,
    int64_t target_micros
) {
    if (time_base_num <= 0 || time_base_den <= 0) {
        return 0;
    }
    int64_t frame_start_micros = av_rescale_q(
        frame_timestamp - timestamp_origin,
        (AVRational){time_base_num, time_base_den},
        (AVRational){1, 1000000}
    );
    if (frame_start_micros <= target_micros) {
        return 0;
    }
    int64_t samples = av_rescale_rnd(
        frame_start_micros - target_micros,
        ENZO_AUDIO_OUTPUT_RATE,
        1000000,
        AV_ROUND_NEAR_INF
    );
    return samples > INT32_MAX ? INT32_MAX : (int)samples;
}

static int write_converted_audio(
    EnzoAudioConverter *converter,
    AVCodecContext *codec,
    AVStream *stream,
    int64_t timestamp_origin,
    AVFrame *frame,
    EnzoPulseOutput *pulse,
    const int *stop_flag,
    const int *pause_flag,
    const int *mute_flag,
    const int *seek_generation,
    const int *released_seek_generation,
    int *buffered_seek_generation,
    int *seen_seek_generation,
    int *corked,
    EnzoAudioSeekState *seek_state,
    EnzoAudioClock *clock,
    int64_t *playback_micros,
    char *err,
    size_t err_len
) {
    if (audio_converter_configure(converter, codec, frame, err, err_len) < 0) {
        return -1;
    }

    int64_t delayed_input_samples = swr_get_delay(converter->swr, converter->src_rate);
    int delayed_output_samples = (int)av_rescale_rnd(
        delayed_input_samples,
        ENZO_AUDIO_OUTPUT_RATE,
        converter->src_rate,
        AV_ROUND_UP
    );
    int out_samples = (int)av_rescale_rnd(
        delayed_input_samples + frame->nb_samples,
        ENZO_AUDIO_OUTPUT_RATE,
        converter->src_rate,
        AV_ROUND_UP
    );
    if (out_samples <= 0) {
        return 0;
    }

    if (out_samples > converter->out_capacity) {
        av_freep(&converter->out_buffer);
        int line_size = 0;
        int ret = av_samples_alloc(
            &converter->out_buffer,
            &line_size,
            ENZO_AUDIO_OUTPUT_CHANNELS,
            out_samples,
            AV_SAMPLE_FMT_S16,
            0
        );
        if (ret < 0) {
            enzo_set_ffmpeg_error(err, err_len, "failed to allocate audio buffer", ret);
            return -1;
        }
        converter->out_capacity = out_samples;
    }

    uint8_t *output_planes[1] = {converter->out_buffer};
    int converted = swr_convert(
        converter->swr,
        output_planes,
        out_samples,
        (const uint8_t **)frame->extended_data,
        frame->nb_samples
    );
    if (converted < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to resample audio", converted);
        return -1;
    }

    int skip_samples = 0;
    int leading_silence_samples = 0;
    if (seek_state->active && frame->best_effort_timestamp != AV_NOPTS_VALUE) {
        leading_silence_samples = enzo_audio_seek_leading_silence_samples(
            frame->best_effort_timestamp,
            timestamp_origin,
            stream->time_base.num,
            stream->time_base.den,
            seek_state->target_micros
        );
        skip_samples = enzo_audio_seek_trim_samples(
            frame->best_effort_timestamp,
            timestamp_origin,
            stream->time_base.num,
            stream->time_base.den,
            frame->nb_samples,
            converter->src_rate,
            seek_state->target_micros,
            delayed_output_samples,
            converted
        );
        if (skip_samples < 0) {
            return 0;
        }
        seek_state->active = 0;
    } else if (seek_state->active) {
        seek_state->active = 0;
    }

    int output_samples = converted - skip_samples;
    int bytes = av_samples_get_buffer_size(
        NULL,
        ENZO_AUDIO_OUTPUT_CHANNELS,
        output_samples,
        AV_SAMPLE_FMT_S16,
        1
    );
    if (bytes < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to size audio buffer", bytes);
        return -1;
    }

    if (bytes > 0) {
        const int silence_chunk_bytes =
            ENZO_AUDIO_OUTPUT_RATE / 100 * ENZO_AUDIO_OUTPUT_CHANNELS *
            ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE;
        uint8_t silence[silence_chunk_bytes];
        memset(silence, 0, sizeof(silence));
        int64_t silence_bytes =
            (int64_t)leading_silence_samples * ENZO_AUDIO_OUTPUT_CHANNELS *
            ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE;
        while (silence_bytes > 0) {
            int64_t chunk =
                silence_bytes < silence_chunk_bytes ? silence_bytes : silence_chunk_bytes;
            int ret = enzo_pulse_output_write(
                pulse,
                silence,
                (int)chunk,
                stop_flag,
                pause_flag,
                seek_generation,
                released_seek_generation,
                buffered_seek_generation,
                seen_seek_generation,
                corked,
                clock,
                playback_micros,
                err,
                err_len
            );
            if (ret != 0) {
                return ret;
            }
            silence_bytes -= chunk;
        }

        uint8_t *output_data =
            converter->out_buffer +
            skip_samples * ENZO_AUDIO_OUTPUT_CHANNELS * ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE;
        if (enzo_mute_requested(mute_flag)) {
            memset(output_data, 0, (size_t)bytes);
        }
        int ret = enzo_pulse_output_write(
            pulse,
            output_data,
            bytes,
            stop_flag,
            pause_flag,
            seek_generation,
            released_seek_generation,
            buffered_seek_generation,
            seen_seek_generation,
            corked,
            clock,
            playback_micros,
            err,
            err_len
        );
        if (ret < 0) {
            return -1;
        }
        if (ret > 0) {
            return 1;
        }
    }

    return 0;
}

int enzo_play_audio(
    const char *path,
    int audio_stream_index,
    const int *stop_flag,
    const int *pause_flag,
    const int *mute_flag,
    const int *seek_generation,
    const int64_t *seek_micros,
    const int *released_seek_generation,
    int *applied_seek_generation,
    int *buffered_seek_generation,
    int64_t *playback_micros,
    char *err,
    size_t err_len
) {
    enzo_suppress_ffmpeg_logs();

    if (path == NULL) {
        enzo_set_error(err, err_len, "invalid audio path");
        return -1;
    }

    AVFormatContext *format = NULL;
    AVCodecContext *codec = NULL;
    int stream_index = -1;
    int opened = open_audio_decoder(path, audio_stream_index, &format, &codec, &stream_index, err, err_len);
    if (opened <= 0) {
        return opened;
    }
    AVStream *stream = format->streams[stream_index];
    int64_t timestamp_origin = enzo_stream_timestamp_origin(format, stream);

    EnzoAudioConverter converter;
    audio_converter_init(&converter);

    EnzoPulseOutput pulse;
    if (enzo_pulse_output_open(&pulse, err, err_len) < 0) {
        avcodec_free_context(&codec);
        avformat_close_input(&format);
        return -1;
    }

    AVPacket *packet = av_packet_alloc();
    AVFrame *frame = av_frame_alloc();
    int ret = 0;
    int failed = 0;
    int flushing = 0;
    int corked = 1;
    int seen_seek_generation = 0;
    EnzoAudioSeekState seek_state = {
        .active = 0,
        .target_micros = 0,
    };
    EnzoAudioClock clock;
    enzo_audio_clock_reset(&clock, 0, 0, playback_micros);

    if (packet == NULL || frame == NULL) {
        enzo_set_error(err, err_len, "failed to allocate audio packet/frame");
        failed = 1;
    }

decode_audio:
    while (!failed && !enzo_stop_requested(stop_flag)) {
        int seek_status = sync_audio_seek(
            &pulse,
            format,
            codec,
            &converter,
            stream_index,
            packet,
            frame,
            seek_generation,
            seek_micros,
            &seen_seek_generation,
            applied_seek_generation,
            &corked,
            &flushing,
            &seek_state,
            &clock,
            playback_micros,
            err,
            err_len
        );
        if (seek_status < 0) {
            failed = 1;
            break;
        }
        if (seek_status > 0) {
            continue;
        }

        int pause_status = enzo_sync_pulse_pause(
            &pulse,
            stop_flag,
            pause_flag,
            seek_generation,
            released_seek_generation,
            &seen_seek_generation,
            &corked,
            err,
            err_len
        );
        if (pause_status < 0) {
            failed = 1;
            break;
        }
        if (pause_status == 2) {
            continue;
        }
        if (pause_status > 0) {
            break;
        }
        enzo_audio_clock_update(
            &pulse,
            corked,
            released_seek_generation,
            seen_seek_generation,
            &clock,
            playback_micros
        );

        ret = avcodec_receive_frame(codec, frame);
        if (ret == 0) {
            seek_status = sync_audio_seek(
                &pulse,
                format,
                codec,
                &converter,
                stream_index,
                packet,
                frame,
                seek_generation,
                seek_micros,
                &seen_seek_generation,
                applied_seek_generation,
                &corked,
                &flushing,
                &seek_state,
                &clock,
                playback_micros,
                err,
                err_len
            );
            if (seek_status < 0) {
                failed = 1;
                av_frame_unref(frame);
                break;
            }
            if (seek_status > 0) {
                continue;
            }

            pause_status = enzo_sync_pulse_pause(
                &pulse,
                stop_flag,
                pause_flag,
                seek_generation,
                released_seek_generation,
                &seen_seek_generation,
                &corked,
                err,
                err_len
            );
            if (pause_status < 0) {
                failed = 1;
                av_frame_unref(frame);
                break;
            }
            if (pause_status == 2) {
                av_frame_unref(frame);
                continue;
            }
            if (pause_status > 0) {
                av_frame_unref(frame);
                break;
            }
            int write_status = write_converted_audio(
                &converter,
                codec,
                stream,
                timestamp_origin,
                frame,
                &pulse,
                stop_flag,
                pause_flag,
                mute_flag,
                seek_generation,
                released_seek_generation,
                buffered_seek_generation,
                &seen_seek_generation,
                &corked,
                &seek_state,
                &clock,
                playback_micros,
                err,
                err_len
            );
            if (write_status < 0) {
                failed = 1;
            } else if (write_status > 0) {
                av_frame_unref(frame);
                continue;
            }
            av_frame_unref(frame);
            continue;
        }
        if (ret == AVERROR_EOF) {
            break;
        }
        if (ret != AVERROR(EAGAIN)) {
            enzo_set_ffmpeg_error(err, err_len, "failed to receive audio frame", ret);
            failed = 1;
            break;
        }

        if (flushing) {
            break;
        }

        ret = av_read_frame(format, packet);
        if (ret == AVERROR_EOF) {
            flushing = 1;
            ret = avcodec_send_packet(codec, NULL);
            if (ret < 0 && ret != AVERROR_EOF) {
                enzo_set_ffmpeg_error(err, err_len, "failed to flush audio decoder", ret);
                failed = 1;
            }
            continue;
        }
        if (ret < 0) {
            enzo_set_ffmpeg_error(err, err_len, "failed to read audio packet", ret);
            failed = 1;
            break;
        }

        if (packet->stream_index == stream_index) {
            ret = avcodec_send_packet(codec, packet);
            av_packet_unref(packet);
            if (ret < 0 && ret != AVERROR(EAGAIN)) {
                enzo_set_ffmpeg_error(err, err_len, "failed to send audio packet", ret);
                failed = 1;
                break;
            }
        } else {
            av_packet_unref(packet);
        }
    }

    if (!failed && !enzo_stop_requested(stop_flag)) {
        enzo_atomic_store_generation(buffered_seek_generation, seen_seek_generation);
        int seek_after_eof = 0;
        while (corked && !enzo_stop_requested(stop_flag)) {
            int pause_status = enzo_sync_pulse_pause(
                &pulse,
                stop_flag,
                pause_flag,
                seek_generation,
                released_seek_generation,
                &seen_seek_generation,
                &corked,
                err,
                err_len
            );
            if (pause_status < 0) {
                failed = 1;
                break;
            }
            if (pause_status == 2) {
                seek_after_eof = 1;
                break;
            }
            if (pause_status > 0) {
                break;
            }
            if (corked) {
                av_usleep(1000);
            }
        }
        if (seek_after_eof) {
            goto decode_audio;
        }
        if (!failed && !enzo_stop_requested(stop_flag) &&
            enzo_pulse_output_drain(&pulse, err, err_len) < 0) {
            failed = 1;
        }
    }

    audio_converter_close(&converter);
    if (frame != NULL) {
        av_frame_free(&frame);
    }
    if (packet != NULL) {
        av_packet_free(&packet);
    }
    enzo_pulse_output_close(&pulse);
    avcodec_free_context(&codec);
    avformat_close_input(&format);
    enzo_atomic_store_micros(playback_micros, -1);

    return failed ? -1 : 0;
}
