#!/usr/bin/env sh
set -eu

REPO="EeroEternal/unigateway"
BIN_NAME="ug"

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

log() {
  printf '%s\n' "$*"
}

fail() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os_part="apple-darwin" ;;
    Linux) os_part="unknown-linux-gnu" ;;
    *) fail "unsupported OS: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64) arch_part="x86_64" ;;
    arm64|aarch64) arch_part="aarch64" ;;
    *) fail "unsupported architecture: $arch" ;;
  esac

  target="${arch_part}-${os_part}"

  # Current release matrix does not include Linux aarch64 artifacts.
  if [ "$target" = "aarch64-unknown-linux-gnu" ]; then
    fail "no prebuilt release for $target yet; please use cargo install unigateway"
  fi

  printf '%s' "$target"
}

resolve_install_dir() {
  if [ -n "${UG_INSTALL_DIR:-}" ]; then
    printf '%s' "$UG_INSTALL_DIR"
    return
  fi

  if [ -d "/opt/homebrew/bin" ]; then
    printf '%s' "/opt/homebrew/bin"
    return
  fi

  printf '%s' "/usr/local/bin"
}

verify_sha256() {
  archive_path="$1"
  checksum_path="$2"

  expected="$(awk '{print $1}' "$checksum_path")"
  if [ -z "$expected" ]; then
    fail "checksum file is empty"
  fi

  if command_exists shasum; then
    actual="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
  elif command_exists sha256sum; then
    actual="$(sha256sum "$archive_path" | awk '{print $1}')"
  else
    fail "missing sha256 tool (requires shasum or sha256sum)"
  fi

  if [ "$expected" != "$actual" ]; then
    fail "checksum mismatch for $archive_path"
  fi
}

install_binary() {
  src="$1"
  dst_dir="$2"
  dst="$dst_dir/$BIN_NAME"

  mkdir -p "$dst_dir"

  if [ -w "$dst_dir" ]; then
    install -m 0755 "$src" "$dst"
    printf '%s' "$dst"
    return
  fi

  if command_exists sudo; then
    sudo install -m 0755 "$src" "$dst"
    printf '%s' "$dst"
    return
  fi

  fallback_dir="$HOME/.local/bin"
  mkdir -p "$fallback_dir"
  install -m 0755 "$src" "$fallback_dir/$BIN_NAME"
  printf '%s' "$fallback_dir/$BIN_NAME"
}

main() {
  command_exists curl || fail "curl is required"
  command_exists tar || fail "tar is required"
  command_exists install || fail "install is required"

  version="${UG_VERSION:-latest}"
  target="$(detect_target)"
  asset="${BIN_NAME}-${target}.tar.gz"
  install_dir="$(resolve_install_dir)"

  if [ "$version" = "latest" ]; then
    base_url="https://github.com/${REPO}/releases/latest/download"
  else
    case "$version" in
      v*) tag="$version" ;;
      *) tag="v$version" ;;
    esac
    base_url="https://github.com/${REPO}/releases/download/${tag}"
  fi

  archive_url="${base_url}/${asset}"
  checksum_url="${archive_url}.sha256"

  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT INT TERM

  archive_path="$tmp_dir/$asset"
  checksum_path="$tmp_dir/$asset.sha256"

  log "Downloading ${asset}..."
  curl -fsSL "$archive_url" -o "$archive_path"
  curl -fsSL "$checksum_url" -o "$checksum_path"

  verify_sha256 "$archive_path" "$checksum_path"

  tar -xzf "$archive_path" -C "$tmp_dir"
  [ -f "$tmp_dir/$BIN_NAME" ] || fail "archive does not contain $BIN_NAME"

  installed_path="$(install_binary "$tmp_dir/$BIN_NAME" "$install_dir")"

  log "Installed $BIN_NAME to $installed_path"
  log "Run '$BIN_NAME --help' to get started."
}

main "$@"