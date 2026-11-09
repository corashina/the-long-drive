#!/bin/sh
# Builds the browser package in web/pkg. Runs in the toolchain container.
# Needs PROFILE (release or fast) and CARGO_TARGET_DIR.
set -eu

wasm="$CARGO_TARGET_DIR/wasm32-unknown-unknown/$PROFILE"

# Check the exported reference manifest natively before the WASM build.
cargo run --quiet --locked --bin validate-reference -- web/assets/reference/manifest.bin.gz

cargo build --locked --profile "$PROFILE" --target wasm32-unknown-unknown --lib \
    -p thelongdrive -p simulation-worker -p crunch-worker
# Road and grass admit a 2 MiB WASM memory first, so the mesh worker starts at that size.
cargo rustc --locked --profile "$PROFILE" --target wasm32-unknown-unknown --lib -p mesh-worker \
    -- -C link-arg=--initial-memory=2097152

mkdir -p web/pkg
for module in thelongdrive simulation_worker mesh_worker crunch_worker; do
    wasm-bindgen --target web --out-dir web/pkg --out-name "$module" "$wasm/$module.wasm"
done

# Fast builds skip wasm-opt.
[ "$PROFILE" = fast ] && exit 0

pids=""
for module in web/pkg/*_bg.wasm; do
    wasm-opt -O3 --enable-simd --enable-bulk-memory --enable-bulk-memory-opt \
        --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals \
        --enable-reference-types --enable-multivalue --enable-call-indirect-overlong \
        "$module" -o "$module" &
    pids="$pids $!"
done
for pid in $pids; do
    wait "$pid"
done
