#!/usr/bin/env bash
# Environment for the zgraph spike.
#
# Why a private CARGO_HOME: this session's file sandbox only allows writes under
# the workspace, and cargo insists on writing the registry cache under
# $CARGO_HOME. Pointing CARGO_HOME inside _spike keeps the build hermetic and
# does not touch the host's ~/.cargo.
#
# Why the proxy: direct connections to static.crates.io / index.crates.io time
# out on this machine; the local Clash proxy (127.0.0.1:7890) works.

SPIKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export SPIKE_DIR
export CARGO_HOME="$SPIKE_DIR/.cargo-home"
export PYO3_PYTHON=/opt/homebrew/bin/python3
export HTTPS_PROXY=http://127.0.0.1:7890
export HTTP_PROXY=http://127.0.0.1:7890
export CARGO_NET_RETRY=3
