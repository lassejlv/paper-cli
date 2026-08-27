#!/bin/sh

set -eu

repository="lassejlv/paper-cli"
install_dir="${PAPER_INSTALL_DIR:-$HOME/.local/bin}"
version="${PAPER_VERSION:-latest}"

say() {
    printf '%s\n' "$*"
}

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

command -v tar >/dev/null 2>&1 || fail "\`tar\` is required"

case "$(uname -s)" in
    Darwin)
        platform="apple-darwin"
        ;;
    Linux)
        platform="unknown-linux-gnu"
        ;;
    *)
        fail "unsupported operating system: $(uname -s); use the Windows release archive or install with Cargo"
        ;;
esac

case "$(uname -m)" in
    x86_64 | amd64)
        architecture="x86_64"
        ;;
    arm64 | aarch64)
        architecture="aarch64"
        ;;
    *)
        fail "unsupported CPU architecture: $(uname -m)"
        ;;
esac

target="${architecture}-${platform}"
archive="paper-${target}.tar.gz"

case "$version" in
    latest)
        download_root="https://github.com/${repository}/releases/latest/download"
        ;;
    v[0-9]*)
        download_root="https://github.com/${repository}/releases/download/${version}"
        ;;
    [0-9]*)
        version="v${version}"
        download_root="https://github.com/${repository}/releases/download/${version}"
        ;;
    *)
        fail "invalid PAPER_VERSION \`${version}\`; expected \`latest\` or a version such as \`v0.1.0\`"
        ;;
esac

temporary_dir="$(mktemp -d 2>/dev/null || mktemp -d -t paper-cli)"
cleanup() {
    rm -rf "$temporary_dir"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

download() {
    source_url="$1"
    destination="$2"

    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
            "$source_url" --output "$destination"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --quiet "$source_url" --output-document="$destination"
    else
        fail "\`curl\` or \`wget\` is required"
    fi
}

say "Downloading paper for ${target}..."
download "${download_root}/${archive}" "${temporary_dir}/${archive}"
download "${download_root}/SHA256SUMS" "${temporary_dir}/SHA256SUMS"

awk -v file="$archive" '$2 == file { print $1 "  " $2 }' \
    "${temporary_dir}/SHA256SUMS" >"${temporary_dir}/ASSET_SHA256"

checksum_count="$(wc -l <"${temporary_dir}/ASSET_SHA256" | tr -d '[:space:]')"
[ "$checksum_count" = "1" ] ||
    fail "release checksums do not contain exactly one entry for ${archive}"

if command -v sha256sum >/dev/null 2>&1; then
    (cd "$temporary_dir" && sha256sum --check ASSET_SHA256 >/dev/null)
elif command -v shasum >/dev/null 2>&1; then
    (cd "$temporary_dir" && shasum --algorithm 256 --check ASSET_SHA256 >/dev/null)
else
    fail "\`sha256sum\` or \`shasum\` is required to verify the download"
fi

tar -xzf "${temporary_dir}/${archive}" -C "$temporary_dir" paper
[ -f "${temporary_dir}/paper" ] || fail "release archive does not contain the paper binary"

mkdir -p "$install_dir"
staged_binary="${install_dir}/.paper-install.$$"
install -m 0755 "${temporary_dir}/paper" "$staged_binary"
mv -f "$staged_binary" "${install_dir}/paper"

say "Installed paper to ${install_dir}/paper"
case ":${PATH}:" in
    *":${install_dir}:"*) ;;
    *)
        say "Add ${install_dir} to PATH to run paper from any directory."
        ;;
esac
