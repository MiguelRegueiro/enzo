#!/usr/bin/env bash
set -euo pipefail

readonly FFMPEG_VERSION="9.0"
readonly FFMPEG_SHA256="7f607a00dd0d28a729d5a4811205812eef01cf6ef6155025febb6f36a9062d52"
readonly FFMPEG_ARCHIVE="ffmpeg-${FFMPEG_VERSION}.tar.xz"
readonly FFMPEG_URL="https://ffmpeg.org/releases/${FFMPEG_ARCHIVE}"
readonly DAV1D_VERSION="1.5.4"
readonly DAV1D_SHA256="686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5"
readonly DAV1D_ARCHIVE="dav1d-${DAV1D_VERSION}.tar.xz"
readonly DAV1D_URL="https://downloads.videolan.org/videolan/dav1d/${DAV1D_VERSION}/${DAV1D_ARCHIVE}"

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
work_root="${ENZO_PORTABLE_WORK_DIR:-${repo_root}/target/portable-freebsd}"
mkdir -p "${work_root}"
work_root="$(cd -- "${work_root}" && pwd)"
if [[ "${work_root}" == "/" || "${work_root}" == "${repo_root}" ]]; then
    printf 'unsafe portable work directory: %s\n' "${work_root}" >&2
    exit 1
fi

download_dir="${ENZO_PORTABLE_DOWNLOAD_DIR:-${work_root}/downloads}"
mkdir -p "${download_dir}"
download_dir="$(cd -- "${download_dir}" && pwd)"
source_dir="${work_root}/ffmpeg-source"
dav1d_source_dir="${work_root}/dav1d-source"
dav1d_build_dir="${work_root}/dav1d-build"
managed_prefix_dir="${work_root}/ffmpeg-prefix"
cargo_target_dir="${work_root}/cargo"
build_info_dir="${work_root}/build-info"
binary="${cargo_target_dir}/release/enzo"
jobs="${JOBS:-$(sysctl -n hw.ncpu)}"
ffmpeg_source_input="${ENZO_FFMPEG_SOURCE:-}"
dav1d_source_input="${ENZO_DAV1D_SOURCE:-}"
external_prefix_input="${ENZO_FFMPEG_PREFIX:-}"

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing required command: %s\n' "$1" >&2
        exit 1
    fi
}

resolve_directory() {
    if [[ ! -d "$1" ]]; then
        printf 'directory does not exist: %s\n' "$1" >&2
        exit 1
    fi
    (cd -- "$1" && pwd)
}

copy_source_tree() {
    local input="$1"
    local destination="$2"
    mkdir -p "${destination}"
    cp -a -- "${input}/." "${destination}/"
}

for command in cargo cc cp dirname find git gmake ldd pkg-config rustc sed stat sysctl; do
    require_command "${command}"
done

sha256sum_command="$(command -v gsha256sum || command -v sha256sum || true)"
readelf_command="$(command -v greadelf || command -v readelf || true)"
if [[ -z "${sha256sum_command}" ]]; then
    printf 'missing required command: sha256sum or gsha256sum\n' >&2
    exit 1
fi
if [[ -z "${readelf_command}" ]]; then
    printf 'missing required command: readelf or greadelf\n' >&2
    exit 1
fi

sha256sum() {
    "${sha256sum_command}" "$@"
}

readelf() {
    "${readelf_command}" "$@"
}

for dependency in gnutls libpulse freetype2 harfbuzz fribidi zlib liblzma; do
    if ! pkg-config --exists "${dependency}"; then
        printf 'missing required pkg-config dependency: %s\n' "${dependency}" >&2
        exit 1
    fi
done

if [[ -n "${external_prefix_input}" && ( -n "${ffmpeg_source_input}" || -n "${dav1d_source_input}" ) ]]; then
    printf 'ENZO_FFMPEG_PREFIX cannot be combined with ENZO_FFMPEG_SOURCE or ENZO_DAV1D_SOURCE\n' >&2
    exit 1
fi

if [[ -n "${external_prefix_input}" ]]; then
    prefix_dir="$(resolve_directory "${external_prefix_input}")"
    prefix_pkgconfig_path=""
    for dependency in libavformat libavcodec libavfilter libavutil libswscale libswresample; do
        dependency_pc="$(find "${prefix_dir}" -type f -name "${dependency}.pc" -print -quit)"
        if [[ -z "${dependency_pc}" ]]; then
            printf 'external FFmpeg prefix does not contain pkg-config metadata for %s\n' \
                "${dependency}" >&2
            exit 1
        fi
        dependency_pc_dir="$(dirname "${dependency_pc}")"
        case ":${prefix_pkgconfig_path}:" in
            *":${dependency_pc_dir}:"*) ;;
            *) prefix_pkgconfig_path="${prefix_pkgconfig_path:+${prefix_pkgconfig_path}:}${dependency_pc_dir}" ;;
        esac
    done
    export PKG_CONFIG_PATH="${prefix_pkgconfig_path}${PKG_CONFIG_PATH:+:${PKG_CONFIG_PATH}}"

    for dependency in libavformat libavcodec libavfilter libavutil libswscale libswresample; do
        if ! pkg-config --exists "${dependency}"; then
            printf 'external FFmpeg prefix cannot resolve %s or its private dependencies\n' \
                "${dependency}" >&2
            exit 1
        fi

        dependency_libdir="$(pkg-config --variable=libdir "${dependency}")"
        if [[ "${dependency_libdir}" != "${prefix_dir}/lib" \
            && "${dependency_libdir}" != "${prefix_dir}/lib64" ]]; then
            printf 'pkg-config metadata for %s points outside the external prefix: %s\n' \
                "${dependency}" "${dependency_libdir}" >&2
            exit 1
        fi
        if [[ ! -f "${dependency_libdir}/${dependency}.a" ]]; then
            printf 'external FFmpeg prefix does not contain static %s\n' \
                "${dependency_libdir}/${dependency}.a" >&2
            exit 1
        fi
    done

    rm -rf "${cargo_target_dir}" "${build_info_dir}"
    mkdir -p "${build_info_dir}"
    printf 'FFmpeg prefix: %s\n' "${prefix_dir}" > "${build_info_dir}/source-inputs.txt"
else
    for command in curl make meson nasm ninja tar; do
        require_command "${command}"
    done

    if [[ -n "${ffmpeg_source_input}" ]]; then
        ffmpeg_source_input="$(resolve_directory "${ffmpeg_source_input}")"
        if [[ ! -x "${ffmpeg_source_input}/configure" ]]; then
            printf 'FFmpeg source does not contain an executable configure script: %s\n' "${ffmpeg_source_input}" >&2
            exit 1
        fi
    fi
    if [[ -n "${dav1d_source_input}" ]]; then
        dav1d_source_input="$(resolve_directory "${dav1d_source_input}")"
        if [[ ! -f "${dav1d_source_input}/meson.build" ]]; then
            printf 'dav1d source does not contain meson.build: %s\n' "${dav1d_source_input}" >&2
            exit 1
        fi
    fi

    case "${ffmpeg_source_input}" in
        "${work_root}"|"${work_root}"/*)
            printf 'ENZO_FFMPEG_SOURCE must be outside the portable work directory\n' >&2
            exit 1
            ;;
    esac
    case "${dav1d_source_input}" in
        "${work_root}"|"${work_root}"/*)
            printf 'ENZO_DAV1D_SOURCE must be outside the portable work directory\n' >&2
            exit 1
            ;;
    esac

    rm -rf "${cargo_target_dir}" "${build_info_dir}"
    mkdir -p "${build_info_dir}"

    archive="${download_dir}/${FFMPEG_ARCHIVE}"
    if [[ -z "${ffmpeg_source_input}" ]]; then
        if [[ ! -f "${archive}" ]]; then
            curl --fail --location --retry 3 --output "${archive}" "${FFMPEG_URL}"
        fi
        printf '%s  %s\n' "${FFMPEG_SHA256}" "${archive}" | sha256sum --check --status
    fi

    dav1d_archive="${download_dir}/${DAV1D_ARCHIVE}"
    if [[ -z "${dav1d_source_input}" ]]; then
        if [[ ! -f "${dav1d_archive}" ]]; then
            curl --fail --location --retry 3 --output "${dav1d_archive}" "${DAV1D_URL}"
        fi
        printf '%s  %s\n' "${DAV1D_SHA256}" "${dav1d_archive}" | sha256sum --check --status
    fi

    rm -rf "${source_dir}" "${dav1d_source_dir}" "${dav1d_build_dir}" "${managed_prefix_dir}"
    mkdir -p "${source_dir}" "${dav1d_source_dir}" "${managed_prefix_dir}"
    prefix_dir="${managed_prefix_dir}"

    if [[ -n "${ffmpeg_source_input}" ]]; then
        copy_source_tree "${ffmpeg_source_input}" "${source_dir}"
    else
        tar --extract --file "${archive}" --directory "${source_dir}" --strip-components=1
    fi
    if [[ -n "${dav1d_source_input}" ]]; then
        copy_source_tree "${dav1d_source_input}" "${dav1d_source_dir}"
    else
        tar --extract --file "${dav1d_archive}" --directory "${dav1d_source_dir}" --strip-components=1
    fi

    dav1d_meson_args=(
        "--prefix=${prefix_dir}"
        "--libdir=lib"
        "--buildtype=release"
        "--default-library=static"
        "-Denable_tools=false"
        "-Denable_tests=false"
    )
    printf 'meson setup dav1d-build dav1d-source' > "${build_info_dir}/dav1d-build-command.txt"
    printf ' %q' "${dav1d_meson_args[@]}" >> "${build_info_dir}/dav1d-build-command.txt"
    printf '\n' >> "${build_info_dir}/dav1d-build-command.txt"

    meson setup "${dav1d_build_dir}" "${dav1d_source_dir}" "${dav1d_meson_args[@]}"
    ninja -C "${dav1d_build_dir}" >/dev/null
    ninja -C "${dav1d_build_dir}" install >/dev/null

    dav1d_pc="$(find "${prefix_dir}" -type f -name dav1d.pc -print -quit)"
    if [[ -z "${dav1d_pc}" ]]; then
        printf 'dav1d installation did not produce pkg-config metadata under %s\n' \
            "${prefix_dir}" >&2
        exit 1
    fi
    private_pkgconfig_dir="$(dirname "${dav1d_pc}")"
    export PKG_CONFIG_PATH="${private_pkgconfig_dir}${PKG_CONFIG_PATH:+:${PKG_CONFIG_PATH}}"
    if ! pkg-config --exists 'dav1d >= 1.0.0'; then
        printf 'installed dav1d pkg-config metadata is not resolvable: %s\n' \
            "${dav1d_pc}" >&2
        exit 1
    fi

    ffmpeg_configure_args=(
        "--prefix=${prefix_dir}"
        "--cc=cc"
        "--cxx=c++"
        "--disable-autodetect"
        "--disable-debug"
        "--disable-doc"
        "--disable-gpl"
        "--disable-nonfree"
        "--disable-programs"
        "--disable-shared"
        "--enable-gnutls"
        "--enable-libdav1d"
        "--enable-lzma"
        "--enable-pic"
        "--enable-static"
        "--enable-zlib"
    )
    printf './configure' > "${build_info_dir}/ffmpeg-configure-command.txt"
    printf ' %q' "${ffmpeg_configure_args[@]}" >> "${build_info_dir}/ffmpeg-configure-command.txt"
    printf '\n' >> "${build_info_dir}/ffmpeg-configure-command.txt"

    pushd "${source_dir}" >/dev/null
    if ! ./configure "${ffmpeg_configure_args[@]}"; then
        printf 'dav1d pkg-config metadata:\n' >&2
        pkg-config --debug --print-errors --static --cflags --libs dav1d >&2 || true
        tail -n 120 ffbuild/config.log >&2 || true
        exit 1
    fi

    config_gpl=""
    config_nonfree=""
    while read -r directive name value; do
        case "${directive} ${name}" in
            '#define CONFIG_GPL') config_gpl="${value}" ;;
            '#define CONFIG_NONFREE') config_nonfree="${value}" ;;
        esac
    done < config.h
    if [[ "${config_gpl}" != "0" ]]; then
        printf 'FFmpeg unexpectedly enabled GPL components\n' >&2
        exit 1
    fi
    if [[ "${config_nonfree}" != "0" ]]; then
        printf 'FFmpeg unexpectedly enabled nonfree components\n' >&2
        exit 1
    fi

    cp config.h "${build_info_dir}/ffmpeg-config.h"
    cp ffbuild/config.log "${build_info_dir}/ffmpeg-config.log"
    gmake -j"${jobs}" >/dev/null
    gmake install >/dev/null
    popd >/dev/null

    ffmpeg_pc="$(find "${prefix_dir}" -type f -name libavformat.pc -print -quit)"
    if [[ -z "${ffmpeg_pc}" ]]; then
        printf 'FFmpeg installation did not produce pkg-config metadata under %s\n' \
            "${prefix_dir}" >&2
        exit 1
    fi
    ffmpeg_pkgconfig_dir="$(dirname "${ffmpeg_pc}")"
    case ":${PKG_CONFIG_PATH}:" in
        *":${ffmpeg_pkgconfig_dir}:"*) ;;
        *) export PKG_CONFIG_PATH="${ffmpeg_pkgconfig_dir}:${PKG_CONFIG_PATH}" ;;
    esac

    meson introspect "${dav1d_build_dir}" --buildoptions > "${build_info_dir}/dav1d-build-options.json"
    {
        if [[ -n "${ffmpeg_source_input}" ]]; then
            printf 'FFmpeg source: %s\n' "${ffmpeg_source_input}"
        else
            printf 'FFmpeg source: %s\n' "${FFMPEG_URL}"
            printf 'FFmpeg SHA-256: %s\n' "${FFMPEG_SHA256}"
        fi
        if [[ -n "${dav1d_source_input}" ]]; then
            printf 'dav1d source: %s\n' "${dav1d_source_input}"
        else
            printf 'dav1d source: %s\n' "${DAV1D_URL}"
            printf 'dav1d SHA-256: %s\n' "${DAV1D_SHA256}"
        fi
    } > "${build_info_dir}/source-inputs.txt"
fi

{
    printf 'Enzo commit: '
    git -C "${repo_root}" rev-parse HEAD 2>/dev/null || printf 'unknown\n'
    rustc -vV
    cargo --version
    cc --version | sed -n '1p'
    pkg-config --version
    meson --version 2>/dev/null || true
    ninja --version 2>/dev/null || true
    nasm -v 2>/dev/null || true
} > "${build_info_dir}/toolchain.txt"

export ENZO_FFMPEG_LINK=static
export CARGO_TARGET_DIR="${cargo_target_dir}"
cargo build --locked --release

shared_dependencies="$(ldd "${binary}")"
if [[ "${shared_dependencies}" == *'not found'* ]]; then
    printf 'portable Enzo has unresolved shared dependencies\n%s\n' \
        "${shared_dependencies}" >&2
    exit 1
fi
while IFS= read -r dependency; do
    case "${dependency}" in
        *libdav1d.so*|*libavcodec.so*|*libavfilter.so*|*libavformat.so*|*libavutil.so*|*libswresample.so*|*libswscale.so*)
            printf 'portable Enzo still depends on shared FFmpeg or dav1d libraries\n%s\n' \
                "${shared_dependencies}" >&2
            exit 1
            ;;
    esac
done <<< "${shared_dependencies}"

dynamic_dependencies="$(readelf --dynamic "${binary}")"
case "${dynamic_dependencies}" in
    *libdav1d.so*|*libavcodec.so*|*libavfilter.so*|*libavformat.so*|*libavutil.so*|*libswresample.so*|*libswscale.so*)
        printf 'portable Enzo records a direct shared FFmpeg or dav1d dependency\n%s\n' \
            "${dynamic_dependencies}" >&2
        exit 1
        ;;
esac

printf '%s\n' "${shared_dependencies}" > "${build_info_dir}/ldd.txt"
printf '%s\n' "${dynamic_dependencies}" > "${build_info_dir}/readelf-dynamic.txt"
pkg-config --static --libs libavformat libavcodec libavfilter libavutil libswscale libswresample \
    > "${build_info_dir}/ffmpeg-static-libs.txt"

printf 'portable binary: %s\n' "${binary}"
if [[ -n "${external_prefix_input}" ]]; then
    printf 'FFmpeg source: external prefix %s\n' "${prefix_dir}"
else
    printf 'FFmpeg version: %s\n' "${FFMPEG_VERSION}"
    printf 'dav1d version: %s\n' "${DAV1D_VERSION}"
    printf 'FFmpeg license mode: LGPL (GPL and nonfree disabled)\n'
fi
printf 'build records: %s\n' "${build_info_dir}"
printf 'portable size: %s bytes\n' "$(stat -f '%z' "${binary}")"
printf '%s\n' "${shared_dependencies}"
