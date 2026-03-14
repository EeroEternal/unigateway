#!/usr/bin/env bash
set -euo pipefail

REPO="EeroEternal/unigateway"
BIN="ug"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
TMPDIR_INSTALL=""

get_latest_tag() {
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | head -1 | cut -d '"' -f4
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin)
      case "$arch" in
        arm64|aarch64) echo "aarch64-apple-darwin" ;;
        x86_64)        echo "x86_64-apple-darwin" ;;
        *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    Linux)
      case "$arch" in
        x86_64) echo "x86_64-unknown-linux-gnu" ;;
        *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    *) echo "Unsupported OS: $os" >&2; exit 1 ;;
  esac
}

cleanup() { [ -n "$TMPDIR_INSTALL" ] && rm -rf "$TMPDIR_INSTALL"; }

main() {
  local tag target archive url

  tag="${1:-$(get_latest_tag)}"
  if [ -z "$tag" ]; then
    echo "Error: could not determine latest release." >&2
    exit 1
  fi

  target="$(detect_target)"
  archive="${BIN}-${target}.tar.gz"
  url="https://github.com/${REPO}/releases/download/${tag}/${archive}"

  echo "Installing ${BIN} ${tag} (${target})..."

  TMPDIR_INSTALL="$(mktemp -d)"
  trap cleanup EXIT

  curl -fsSL "$url" -o "${TMPDIR_INSTALL}/${archive}"
  tar xzf "${TMPDIR_INSTALL}/${archive}" -C "$TMPDIR_INSTALL"

  if [ -w "$INSTALL_DIR" ]; then
    mv "${TMPDIR_INSTALL}/${BIN}" "${INSTALL_DIR}/${BIN}"
  else
    echo "Need sudo to install to ${INSTALL_DIR}"
    sudo mv "${TMPDIR_INSTALL}/${BIN}" "${INSTALL_DIR}/${BIN}"
  fi

  chmod +x "${INSTALL_DIR}/${BIN}"
  echo "Installed ${BIN} to ${INSTALL_DIR}/${BIN}"
  "${INSTALL_DIR}/${BIN}" --version
}

main "$@"
