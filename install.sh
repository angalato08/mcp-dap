#!/usr/bin/env sh
set -eu

REPO="angalato08/mcp-dap"
BINARY="mcp-dap"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

main() {
    platform="$(uname -s)"
    arch="$(uname -m)"

    case "$platform" in
        Linux)  os="unknown-linux-musl" ;;
        Darwin) os="apple-darwin" ;;
        *)      err "Unsupported platform: $platform" ;;
    esac

    case "$arch" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)  arch="aarch64" ;;
        *)              err "Unsupported architecture: $arch" ;;
    esac

    target="${arch}-${os}"

    if [ -n "${VERSION:-}" ]; then
        tag="v$VERSION"
    else
        tag="$(get_latest_tag)"
    fi

    url="https://github.com/${REPO}/releases/download/${tag}/${BINARY}-${target}"

    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    echo "Downloading ${BINARY} ${tag} for ${target}..."
    download "$url" "$tmpdir/$BINARY"
    chmod +x "$tmpdir/$BINARY"

    if [ -w "$INSTALL_DIR" ]; then
        mv "$tmpdir/$BINARY" "$INSTALL_DIR/$BINARY"
    else
        echo "Installing to ${INSTALL_DIR} (requires sudo)..."
        sudo mv "$tmpdir/$BINARY" "$INSTALL_DIR/$BINARY"
    fi

    echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
}

get_latest_tag() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" |
            sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p'
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" |
            sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p'
    else
        err "curl or wget is required"
    fi
}

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$2" "$1"
    else
        err "curl or wget is required"
    fi
}

err() {
    echo "Error: $1" >&2
    exit 1
}

main
