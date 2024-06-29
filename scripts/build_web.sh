#!/usr/bin/env bash
set -eu
script_path=$( cd "$(dirname "${BASH_SOURCE[0]}")" ; pwd -P )
cd "$script_path/.."

./scripts/setup_web.sh

# This is required to enable the web_sys clipboard API which eframe web uses
# https://rustwasm.github.io/wasm-bindgen/api/web_sys/struct.Clipboard.html
# https://rustwasm.github.io/docs/wasm-bindgen/web-sys/unstable-apis.html
export RUSTFLAGS=--cfg=web_sys_unstable_apis

CRATE_NAME="json-editor"

FEATURES="all-features"

OPEN=false
OPTIMIZE=false
BUILD=release
BUILD_FLAGS="--release"
WGPU=true
WASM_OPT_FLAGS="-O2 --fast-math -g"

OUT_FILE_NAME="json-editor"

FINAL_WASM_PATH=web/${OUT_FILE_NAME}.wasm

# Clear output from old stuff:
rm -f "${FINAL_WASM_PATH}"

echo "Building rust…"
echo "cargo build ${BUILD_FLAGS}"
cargo build \
    ${BUILD_FLAGS} \
    --bin ${OUT_FILE_NAME} \
    --target wasm32-unknown-unknown

# Get the output directory (in the workspace it is in another location)
# TARGET=`cargo metadata --format-version=1 | jq --raw-output .target_directory`
TARGET="target"

echo "Generating JS bindings for wasm…"
TARGET_NAME="${CRATE_NAME}.wasm"
WASM_PATH="${TARGET}/wasm32-unknown-unknown/$BUILD/$TARGET_NAME"
wasm-bindgen "${WASM_PATH}" --out-dir web --out-name ${OUT_FILE_NAME} --no-modules --no-typescript

if [[ "${OPTIMIZE}" = true ]]; then
  echo "Optimizing wasm…"
  # to get wasm-opt:  apt/brew/dnf install binaryen
  wasm-opt "${FINAL_WASM_PATH}" $WASM_OPT_FLAGS -o "${FINAL_WASM_PATH}"
fi

echo "Finished ${FINAL_WASM_PATH}"