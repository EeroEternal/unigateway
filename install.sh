#!/usr/bin/env sh
set -eu

printf '%s\n' "UniGateway no longer ships a standalone \`ug\` binary from this repository."
printf '%s\n' "Embed the Rust libraries instead, for example:"
printf '%s\n' "  cargo add unigateway-sdk"
printf '%s\n' "See https://github.com/EeroEternal/unigateway/blob/main/README.md"
exit 1
