// Native modules are listed explicitly so Cargo rebuilds on every C source change.
const MEDIA_SOURCES: &[&str] = &[
    "csrc/common.c",
    "csrc/input.c",
    "csrc/fingerprint.c",
    "csrc/probe.c",
    "csrc/subtitle_decoder.c",
    "csrc/text_layout.c",
    "csrc/video_decoder.c",
    "csrc/audio_output.c",
    "csrc/audio_player.c",
];

const MEDIA_HEADERS: &[&str] = &[
    "csrc/media.h",
    "csrc/internal.h",
    "csrc/input.h",
    "csrc/audio_output.h",
    "csrc/text_layout.h",
];

const PKG_CONFIG_LIBRARIES: &[&str] = &[
    "libavformat",
    "libavcodec",
    "libavfilter",
    "libswscale",
    "libswresample",
    "libavutil",
    "libpulse",
    "freetype2",
    "harfbuzz",
    "fribidi",
];

const LINUX_LINK_LIBRARIES: &[&str] = &[
    "avformat",
    "avcodec",
    "avfilter",
    "avutil",
    "swscale",
    "swresample",
    "pulse",
    "freetype",
    "harfbuzz",
    "fribidi",
];

fn main() {
    println!("cargo:rerun-if-env-changed=ENZO_FFMPEG_LINK");

    let static_ffmpeg = match std::env::var("ENZO_FFMPEG_LINK").as_deref() {
        Ok("static") => true,
        Ok("system") | Err(_) => false,
        Ok(value) => {
            panic!("unsupported ENZO_FFMPEG_LINK value `{value}`; expected `system` or `static`")
        }
    };

    let mut build = cc::Build::new();
    build.include("csrc");

    let native_libraries = probe_native_libraries(static_ffmpeg);
    if let Some(native_libraries) = &native_libraries {
        for library in native_libraries {
            for include_path in &library.include_paths {
                build.include(include_path);
            }
            for (name, value) in &library.defines {
                build.define(name, value.as_deref());
            }
        }
    }

    build
        .files(MEDIA_SOURCES)
        .std("c11")
        .warnings(true)
        .extra_warnings(true)
        .pic(true)
        .flag_if_supported("-Wstrict-prototypes")
        .flag_if_supported("-Wmissing-prototypes")
        .flag_if_supported("-Wno-deprecated-declarations")
        .compile("enzo_media");

    for path in MEDIA_SOURCES.iter().chain(MEDIA_HEADERS) {
        println!("cargo:rerun-if-changed={path}");
    }

    if native_libraries.is_some() {
        for library in PKG_CONFIG_LIBRARIES {
            emit_native_library(library, static_ffmpeg && is_ffmpeg_library(library));
        }
    } else {
        for library in LINUX_LINK_LIBRARIES {
            println!("cargo:rustc-link-lib={library}");
        }
    }
}

fn probe_native_libraries(static_ffmpeg: bool) -> Option<Vec<pkg_config::Library>> {
    let libraries = PKG_CONFIG_LIBRARIES
        .iter()
        .map(|library| {
            let mut config = pkg_config::Config::new();
            config.cargo_metadata(false);
            config.statik(static_ffmpeg && is_ffmpeg_library(library));
            config.probe(library)
        })
        .collect::<Result<Vec<_>, _>>();

    match libraries {
        Ok(libraries) => Some(libraries),
        Err(err) if static_ffmpeg => {
            panic!("failed to find static FFmpeg dependencies with pkg-config: {err}")
        }
        Err(_err) if target_is_linux() => None,
        Err(err) => panic!("failed to find native dependencies with pkg-config/pkgconf: {err}"),
    }
}

fn is_ffmpeg_library(library: &str) -> bool {
    matches!(
        library,
        "libavformat" | "libavcodec" | "libavfilter" | "libswscale" | "libswresample" | "libavutil"
    )
}

fn target_is_linux() -> bool {
    std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|target_os| target_os == "linux")
}

fn emit_native_library(library: &str, statik: bool) {
    let mut config = pkg_config::Config::new();
    config.cargo_metadata(true);
    config.statik(statik);
    config.probe(library).unwrap_or_else(|err| {
        panic!("failed to find native dependency `{library}` with pkg-config/pkgconf: {err}")
    });
}
