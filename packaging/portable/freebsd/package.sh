#!/usr/bin/env bash
set -euo pipefail

readonly FFMPEG_VERSION="9.0"
readonly FFMPEG_SHA256="7f607a00dd0d28a729d5a4811205812eef01cf6ef6155025febb6f36a9062d52"
readonly FFMPEG_ARCHIVE="ffmpeg-${FFMPEG_VERSION}.tar.xz"
readonly DAV1D_VERSION="1.5.4"
readonly DAV1D_SHA256="686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5"
readonly DAV1D_ARCHIVE="dav1d-${DAV1D_VERSION}.tar.xz"

if [[ $# -ne 1 || ! "$1" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    printf 'usage: %s MAJOR.MINOR.PATCH\n' "$0" >&2
    exit 1
fi
version="$1"

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
work_root="${ENZO_PORTABLE_WORK_DIR:-${repo_root}/target/portable-freebsd}"
if [[ ! -d "${work_root}" ]]; then
    printf 'portable work directory does not exist: %s\n' "${work_root}" >&2
    exit 1
fi
work_root="$(cd -- "${work_root}" && pwd)"
if [[ "${work_root}" == "/" || "${work_root}" == "${repo_root}" ]]; then
    printf 'unsafe portable work directory: %s\n' "${work_root}" >&2
    exit 1
fi
download_dir="${ENZO_PORTABLE_DOWNLOAD_DIR:-${work_root}/downloads}"
cargo_target_dir="${work_root}/cargo"
build_info_dir="${work_root}/build-info"
binary="${cargo_target_dir}/release/enzo"
dist_dir="${ENZO_DIST_DIR:-${repo_root}/dist}"

for command in cargo cp git grep gzip gtar install ldd python3 rustc sed; do
    if ! command -v "${command}" >/dev/null 2>&1; then
        printf 'missing required command: %s\n' "${command}" >&2
        exit 1
    fi
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

if [[ -n "$(git -C "${repo_root}" status --porcelain)" ]]; then
    printf 'portable release bundles require a clean Git worktree\n' >&2
    exit 1
fi

manifest_version="$(cargo metadata --format-version=1 --no-deps --manifest-path "${repo_root}/Cargo.toml" \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
if [[ "${manifest_version}" != "${version}" ]]; then
    printf 'requested version %s does not match Cargo.toml version %s\n' "${version}" "${manifest_version}" >&2
    exit 1
fi

for path in \
    "${binary}" \
    "${build_info_dir}/ffmpeg-config.h" \
    "${build_info_dir}/ffmpeg-configure-command.txt" \
    "${build_info_dir}/ffmpeg-config.log" \
    "${build_info_dir}/dav1d-build-command.txt" \
    "${build_info_dir}/dav1d-build-options.json" \
    "${build_info_dir}/source-inputs.txt" \
    "${download_dir}/${FFMPEG_ARCHIVE}" \
    "${download_dir}/${DAV1D_ARCHIVE}"; do
    if [[ ! -f "${path}" ]]; then
        printf 'missing portable release input: %s\n' "${path}" >&2
        exit 1
    fi
done

expected_source_inputs="$(printf '%s\n' \
    'FFmpeg source: https://ffmpeg.org/releases/ffmpeg-9.0.tar.xz' \
    'FFmpeg SHA-256: 7f607a00dd0d28a729d5a4811205812eef01cf6ef6155025febb6f36a9062d52' \
    'dav1d source: https://downloads.videolan.org/videolan/dav1d/1.5.4/dav1d-1.5.4.tar.xz' \
    'dav1d SHA-256: 686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5')"
if [[ "$(<"${build_info_dir}/source-inputs.txt")" != "${expected_source_inputs}" ]]; then
    printf 'release bundles require a binary built from the pinned upstream source archives\n' >&2
    exit 1
fi

printf '%s  %s\n' "${FFMPEG_SHA256}" "${download_dir}/${FFMPEG_ARCHIVE}" | sha256sum --check --status
printf '%s  %s\n' "${DAV1D_SHA256}" "${download_dir}/${DAV1D_ARCHIVE}" | sha256sum --check --status

if ! grep -q '^#define CONFIG_GPL 0$' "${build_info_dir}/ffmpeg-config.h"; then
    printf 'recorded FFmpeg configuration is not LGPL-only: CONFIG_GPL is not 0\n' >&2
    exit 1
fi
if ! grep -q '^#define CONFIG_NONFREE 0$' "${build_info_dir}/ffmpeg-config.h"; then
    printf 'recorded FFmpeg configuration is not redistributable: CONFIG_NONFREE is not 0\n' >&2
    exit 1
fi

shared_dependencies="$(ldd "${binary}")"
if [[ "${shared_dependencies}" == *'not found'* ]]; then
    printf 'portable Enzo has unresolved shared dependencies\n%s\n' \
        "${shared_dependencies}" >&2
    exit 1
fi
case "${shared_dependencies}" in
    *libdav1d.so*|*libavcodec.so*|*libavfilter.so*|*libavformat.so*|*libavutil.so*|*libswresample.so*|*libswscale.so*)
        printf 'portable Enzo depends on a shared FFmpeg or dav1d library\n%s\n' \
            "${shared_dependencies}" >&2
        exit 1
        ;;
esac

dynamic_dependencies="$(readelf --dynamic "${binary}")"
case "${dynamic_dependencies}" in
    *libdav1d.so*|*libavcodec.so*|*libavfilter.so*|*libavformat.so*|*libavutil.so*|*libswresample.so*|*libswscale.so*)
        printf 'portable Enzo records a direct shared FFmpeg or dav1d dependency\n%s\n' \
            "${dynamic_dependencies}" >&2
        exit 1
        ;;
esac

commit="$(git -C "${repo_root}" rev-parse HEAD)"
if ! grep -q "^Enzo commit: ${commit}$" "${build_info_dir}/toolchain.txt"; then
    printf 'portable binary build records do not match Git commit %s\n' "${commit}" >&2
    exit 1
fi

host_target="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "${host_target}" ]]; then
    printf 'could not determine Rust host target\n' >&2
    exit 1
fi

bundle="enzo-${version}-${host_target}"
enzo_source="enzo-${version}"
stage_root="${work_root}/package-stage"
source_date_epoch="$(git -C "${repo_root}" show -s --format=%ct HEAD)"

rm -rf "${stage_root}"
mkdir -p \
    "${stage_root}/${bundle}/share/icons" \
    "${stage_root}/${bundle}/compliance/sources/${enzo_source}"

install -m 0755 "${binary}" "${stage_root}/${bundle}/enzo"
install -m 0644 "${repo_root}/README.md" "${stage_root}/${bundle}/README.md"
install -m 0644 "${repo_root}/CHANGELOG.md" "${stage_root}/${bundle}/CHANGELOG.md"
install -m 0644 "${repo_root}/LICENSE" "${stage_root}/${bundle}/LICENSE"
install -m 0644 "${repo_root}/THIRD_PARTY_NOTICES.md" "${stage_root}/${bundle}/THIRD_PARTY_NOTICES.md"
install -m 0644 "${repo_root}/packaging/linux/enzo.desktop" "${stage_root}/${bundle}/share/enzo.desktop"
install -m 0644 "${repo_root}/packaging/portable/freebsd/RELINK.md" "${stage_root}/${bundle}/compliance/RELINK.md"
cp -R "${repo_root}/LICENSES" "${stage_root}/${bundle}/"
cp -R "${repo_root}/packaging/linux/icons/." "${stage_root}/${bundle}/share/icons/"
cp -R "${build_info_dir}" "${stage_root}/${bundle}/compliance/build-info"

git -C "${repo_root}" archive --format=tar HEAD \
    | gtar -xf - -C "${stage_root}/${bundle}/compliance/sources/${enzo_source}"
install -m 0644 "${download_dir}/${FFMPEG_ARCHIVE}" \
    "${stage_root}/${bundle}/compliance/sources/${FFMPEG_ARCHIVE}"
install -m 0644 "${download_dir}/${DAV1D_ARCHIVE}" \
    "${stage_root}/${bundle}/compliance/sources/${DAV1D_ARCHIVE}"

(
    cd "${stage_root}/${bundle}/compliance/sources"
    sha256sum -- "${FFMPEG_ARCHIVE}" "${DAV1D_ARCHIVE}" > SHA256SUMS
)

# Normalize staged modes so the archive is independent of the caller's umask.
chmod -R u=rwX,go=rX "${stage_root}/${bundle}"

mkdir -p "${dist_dir}"
rm -f "${dist_dir}/${bundle}.tar.gz"
gtar \
    --sort=name \
    --mtime="@${source_date_epoch}" \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    -C "${stage_root}" \
    -cf - "${bundle}" \
    | gzip -n > "${dist_dir}/${bundle}.tar.gz"

gzip --test "${dist_dir}/${bundle}.tar.gz"
for entry in \
    "${bundle}/enzo" \
    "${bundle}/README.md" \
    "${bundle}/CHANGELOG.md" \
    "${bundle}/LICENSE" \
    "${bundle}/THIRD_PARTY_NOTICES.md" \
    "${bundle}/LICENSES/FFmpeg-LGPL-2.1-or-later.txt" \
    "${bundle}/LICENSES/dav1d-BSD-2-Clause.txt" \
    "${bundle}/share/enzo.desktop" \
    "${bundle}/share/icons/hicolor/512x512/apps/enzo.png" \
    "${bundle}/compliance/RELINK.md" \
    "${bundle}/compliance/build-info/ffmpeg-config.h" \
    "${bundle}/compliance/build-info/ffmpeg-config.log" \
    "${bundle}/compliance/build-info/ffmpeg-configure-command.txt" \
    "${bundle}/compliance/build-info/dav1d-build-command.txt" \
    "${bundle}/compliance/build-info/dav1d-build-options.json" \
    "${bundle}/compliance/build-info/ldd.txt" \
    "${bundle}/compliance/build-info/readelf-dynamic.txt" \
    "${bundle}/compliance/build-info/source-inputs.txt" \
    "${bundle}/compliance/build-info/toolchain.txt" \
    "${bundle}/compliance/sources/${enzo_source}/Cargo.lock" \
    "${bundle}/compliance/sources/${enzo_source}/rust-toolchain.toml" \
    "${bundle}/compliance/sources/${enzo_source}/packaging/portable/freebsd/build.sh" \
    "${bundle}/compliance/sources/${FFMPEG_ARCHIVE}" \
    "${bundle}/compliance/sources/${DAV1D_ARCHIVE}" \
    "${bundle}/compliance/sources/SHA256SUMS"; do
    if ! gtar -tzf "${dist_dir}/${bundle}.tar.gz" "${entry}" >/dev/null; then
        printf 'portable archive is missing required entry: %s\n' "${entry}" >&2
        exit 1
    fi
done

printf 'portable archive: %s\n' "${dist_dir}/${bundle}.tar.gz"
