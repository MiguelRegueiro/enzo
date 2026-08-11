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
work_root="${ENZO_PORTABLE_WORK_DIR:-${repo_root}/target/portable-linux}"
download_dir="${ENZO_PORTABLE_DOWNLOAD_DIR:-${work_root}/downloads}"
cargo_target_dir="${work_root}/cargo"
build_info_dir="${work_root}/build-info"
binary="${cargo_target_dir}/release/enzo"
dist_dir="${ENZO_DIST_DIR:-${repo_root}/dist}"

for command in cargo cp git grep gzip install ldd python3 readelf rustc sed sha256sum tar; do
    if ! command -v "${command}" >/dev/null 2>&1; then
        printf 'missing required command: %s\n' "${command}" >&2
        exit 1
    fi
done

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
source_bundle="${bundle}-source"
stage_root="${work_root}/package-stage"
source_date_epoch="$(git -C "${repo_root}" show -s --format=%ct HEAD)"

rm -rf "${stage_root}"
mkdir -p "${stage_root}/${bundle}/packaging" "${stage_root}/${source_bundle}/sources"

install -m 0755 "${binary}" "${stage_root}/${bundle}/enzo"
install -m 0644 "${repo_root}/README.md" "${stage_root}/${bundle}/README.md"
install -m 0644 "${repo_root}/CHANGELOG.md" "${stage_root}/${bundle}/CHANGELOG.md"
install -m 0644 "${repo_root}/LICENSE" "${stage_root}/${bundle}/LICENSE"
install -m 0644 "${repo_root}/THIRD_PARTY_NOTICES.md" "${stage_root}/${bundle}/THIRD_PARTY_NOTICES.md"
install -Dm 0644 "${repo_root}/packaging/portable/RELINK.md" "${stage_root}/${bundle}/packaging/portable/RELINK.md"
cp -R "${repo_root}/LICENSES" "${stage_root}/${bundle}/"
cp -R "${repo_root}/packaging/linux" "${stage_root}/${bundle}/packaging/"
cp -R "${build_info_dir}" "${stage_root}/${bundle}/build-info"

mkdir -p "${stage_root}/${source_bundle}/enzo"
git -C "${repo_root}" archive --format=tar HEAD | tar -xf - -C "${stage_root}/${source_bundle}/enzo"
install -m 0644 "${download_dir}/${FFMPEG_ARCHIVE}" "${stage_root}/${source_bundle}/sources/${FFMPEG_ARCHIVE}"
install -m 0644 "${download_dir}/${DAV1D_ARCHIVE}" "${stage_root}/${source_bundle}/sources/${DAV1D_ARCHIVE}"
cp -R "${build_info_dir}" "${stage_root}/${source_bundle}/build-info"

(
    cd "${stage_root}/${source_bundle}/sources"
    sha256sum -- "${FFMPEG_ARCHIVE}" "${DAV1D_ARCHIVE}" > SHA256SUMS
)

mkdir -p "${dist_dir}"
rm -f "${dist_dir}/${bundle}.tar.gz" "${dist_dir}/${source_bundle}.tar.gz"
for archive_root in "${bundle}" "${source_bundle}"; do
    tar \
        --sort=name \
        --mtime="@${source_date_epoch}" \
        --owner=0 \
        --group=0 \
        --numeric-owner \
        -C "${stage_root}" \
        -cf - "${archive_root}" \
        | gzip -n > "${dist_dir}/${archive_root}.tar.gz"
done

gzip --test "${dist_dir}/${bundle}.tar.gz" "${dist_dir}/${source_bundle}.tar.gz"
for entry in \
    "${bundle}/enzo" \
    "${bundle}/LICENSE" \
    "${bundle}/THIRD_PARTY_NOTICES.md" \
    "${bundle}/packaging/portable/RELINK.md" \
    "${bundle}/LICENSES/FFmpeg-LGPL-2.1-or-later.txt" \
    "${bundle}/LICENSES/dav1d-BSD-2-Clause.txt" \
    "${bundle}/build-info/ffmpeg-config.h"; do
    if ! tar -tzf "${dist_dir}/${bundle}.tar.gz" "${entry}" >/dev/null; then
        printf 'binary archive is missing required entry: %s\n' "${entry}" >&2
        exit 1
    fi
done
for entry in \
    "${source_bundle}/enzo/packaging/portable/linux/build.sh" \
    "${source_bundle}/enzo/packaging/portable/RELINK.md" \
    "${source_bundle}/sources/${FFMPEG_ARCHIVE}" \
    "${source_bundle}/sources/${DAV1D_ARCHIVE}" \
    "${source_bundle}/sources/SHA256SUMS" \
    "${source_bundle}/build-info/ffmpeg-config.h"; do
    if ! tar -tzf "${dist_dir}/${source_bundle}.tar.gz" "${entry}" >/dev/null; then
        printf 'source/relink archive is missing required entry: %s\n' "${entry}" >&2
        exit 1
    fi
done

(
    cd "${dist_dir}"
    sha256sum -- "${bundle}.tar.gz" "${source_bundle}.tar.gz" > "${bundle}-SHA256SUMS"
)

printf 'binary archive: %s\n' "${dist_dir}/${bundle}.tar.gz"
printf 'source/relink archive: %s\n' "${dist_dir}/${source_bundle}.tar.gz"
printf 'checksums: %s\n' "${dist_dir}/${bundle}-SHA256SUMS"
