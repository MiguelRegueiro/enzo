const MEDIA_SOURCES: &[&str] = &[
    "csrc/common.c",
    "csrc/fingerprint.c",
    "csrc/probe.c",
    "csrc/subtitle_decoder.c",
    "csrc/video_decoder.c",
    "csrc/audio_output.c",
    "csrc/audio_player.c",
];

const MEDIA_HEADERS: &[&str] = &["csrc/media.h", "csrc/internal.h", "csrc/audio_output.h"];

fn main() {
    let mut build = cc::Build::new();
    build
        .include("csrc")
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

    println!("cargo:rustc-link-lib=avformat");
    println!("cargo:rustc-link-lib=avcodec");
    println!("cargo:rustc-link-lib=avutil");
    println!("cargo:rustc-link-lib=swscale");
    println!("cargo:rustc-link-lib=swresample");
    println!("cargo:rustc-link-lib=pulse");
    println!("cargo:rustc-link-lib=freetype");
}
