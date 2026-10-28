#!/usr/bin/env bash
# Run every spike probe and capture the raw evidence under _spike/logs/.
set -uo pipefail

source "$(dirname "$0")/env.sh"
cd "$SPIKE_DIR"
mkdir -p logs
rm -f logs_t2_tmp.log

# ---- environment facts -----------------------------------------------------
{
  echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "rustc=$(rustc --version)"
  echo "cargo=$(cargo --version)"
  echo "python=$("$PYO3_PYTHON" --version 2>&1) at $PYO3_PYTHON"
  echo "uname=$(uname -a)"
  echo "cpus=$(sysctl -n hw.ncpu)"
  echo "pyo3=0.30.1 tokio=$(grep -A0 '^name = "tokio"' Cargo.lock >/dev/null && awk '/^name = "tokio"/{getline; print $3}' Cargo.lock | tr -d '\"')"
  echo "note=crates.io direct access blocked; HTTPS_PROXY=http://127.0.0.1:7890 used; CARGO_HOME=$CARGO_HOME"
} 2>&1 | tee logs/00_env.log

# ---- build -----------------------------------------------------------------
cargo build --bins 2>&1 | tee logs/01_build.log
if [ "${PIPESTATUS[0]}" -ne 0 ]; then
  echo "BUILD FAILED - see logs/01_build.log"
  exit 1
fi

run() {
  local name=$1
  shift
  echo "===== $name ====="
  "$@" 2>&1 | tee "logs/$name.log"
  echo "exit=${PIPESTATUS[0]}" | tee -a "logs/$name.log"
}

run 10_t1_embed ./target/debug/t1_embed
run 20_t2_gil ./target/debug/t2_gil
run 30_t3_callback ./target/debug/t3_callback
run 40_t4_gate ./target/debug/t4_gate
run 50_t5_errors ./target/debug/t5_errors
run 60_t6_trace ./target/debug/t6_trace
run 70_t7_timeout ./target/debug/t7_timeout
run 71_t7b_interrupt_main ./target/debug/t7b_interrupt main
run 72_t7b_interrupt_worker ./target/debug/t7b_interrupt worker
run 73_t7b_interrupt_async_exc ./target/debug/t7b_interrupt async_exc
run 74_t7b_interrupt_async_exc_sleep ./target/debug/t7b_interrupt async_exc_sleep
run 80_t8_eventloop ./target/debug/t8_eventloop

# ---- compile-fail evidence: &mut state captured by tokio::spawn -------------
{
  echo "===== alias_fail compile-fail evidence ====="
  cd alias_fail || exit 1
  CARGO_TARGET_DIR="$SPIKE_DIR/target" cargo check 2>&1
  echo "exit=$?"
} 2>&1 | tee logs/90_alias_fail_compile.log

echo "ALL_PROBES_DONE"
