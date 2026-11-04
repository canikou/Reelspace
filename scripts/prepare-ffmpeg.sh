#!/usr/bin/env bash
# Linux counterpart of prepare-ffmpeg.ps1: stage a pinned, hash-verified FFmpeg/FFprobe into vendor/ffmpeg.
set -euo pipefail

archive_name="ffmpeg-n8.1.3-32-g760d10603e-linux64-gpl-8.1.tar.xz"
archive_url="https://github.com/canikou/Reelspace/releases/download/vendor-ffmpeg-n8.1.3/$archive_name"
archive_sha256="5bbfd617d5473c91c8f1fdd1dd8507e8fdc6aa6db0e531f65c95a15486ecacb7"
ffmpeg_sha256="d7ce480a2e2a2aceaaf0f389d9d809a18b5253af8b4676840a09c78a40f2aa90"
ffprobe_sha256="7d1d29ca038f2b9964ceed1980389e26a60766c9ba96bbc1552b48b26f0bd28e"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
bin_root="$repo_root/vendor/ffmpeg/bin"
license_path="$repo_root/vendor/ffmpeg/LICENSE.txt"

hash_ok() { [[ -f "$1" ]] && [[ "$(sha256sum "$1" | cut -d' ' -f1)" == "$2" ]]; }

if hash_ok "$bin_root/ffmpeg" "$ffmpeg_sha256" && hash_ok "$bin_root/ffprobe" "$ffprobe_sha256" && [[ -f "$license_path" ]]; then
  echo "Pinned FFmpeg and FFprobe resources are already verified."
  exit 0
fi

scratch="$(mktemp -d "${TMPDIR:-/tmp}/reelspace-ffmpeg-XXXXXX")"
trap 'rm -rf "$scratch"' EXIT

echo "Downloading pinned FFmpeg build..."
curl --fail --location --silent --show-error --output "$scratch/$archive_name" "$archive_url"
hash_ok "$scratch/$archive_name" "$archive_sha256" || { echo "The FFmpeg archive hash did not match the pinned SHA-256 value." >&2; exit 1; }

tar -xJf "$scratch/$archive_name" -C "$scratch"
root="$scratch/${archive_name%.tar.xz}"
hash_ok "$root/bin/ffmpeg" "$ffmpeg_sha256" || { echo "The extracted ffmpeg hash did not match." >&2; exit 1; }
hash_ok "$root/bin/ffprobe" "$ffprobe_sha256" || { echo "The extracted ffprobe hash did not match." >&2; exit 1; }

mkdir -p "$bin_root"
install -m 0755 "$root/bin/ffmpeg" "$root/bin/ffprobe" "$bin_root/"
install -m 0644 "$root/LICENSE.txt" "$license_path"
echo "Pinned FFmpeg resources verified and staged."
