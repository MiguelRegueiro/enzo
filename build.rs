use std::{env, path::PathBuf, process::Command};

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should be set"));
    let object = out_dir.join("verno_media.o");
    let library = out_dir.join("libverno_media.a");

    let cc_status = Command::new("cc")
        .args([
            "-O3",
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Wno-deprecated-declarations",
            "-fPIC",
            "-c",
            "native/media.c",
            "-o",
        ])
        .arg(&object)
        .status()
        .expect("failed to start C compiler");
    if !cc_status.success() {
        panic!("failed to compile native/media.c");
    }

    let ar_status = Command::new("ar")
        .args(["crs"])
        .arg(&library)
        .arg(&object)
        .status()
        .expect("failed to start ar");
    if !ar_status.success() {
        panic!("failed to archive native media shim");
    }

    println!("cargo:rerun-if-changed=native/media.c");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=verno_media");
    println!("cargo:rustc-link-lib=avformat");
    println!("cargo:rustc-link-lib=avcodec");
    println!("cargo:rustc-link-lib=avutil");
    println!("cargo:rustc-link-lib=swscale");
    println!("cargo:rustc-link-lib=swresample");
    println!("cargo:rustc-link-lib=pulse");
    println!("cargo:rustc-link-lib=freetype");
}
