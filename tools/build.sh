#!/bin/sh
# Builds the browser package in web/pkg. Runs in the toolchain container.
# Needs PROFILE (release or fast) and CARGO_TARGET_DIR.
set -eu

wasm="$CARGO_TARGET_DIR/wasm32-unknown-unknown/$PROFILE"

cargo build --locked --profile "$PROFILE" --target wasm32-unknown-unknown --lib \
    -p the-long-slop -p simulation-worker -p terrain-worker
for crate in road-worker grass-worker; do
    cargo rustc --locked --profile "$PROFILE" --target wasm32-unknown-unknown --lib -p "$crate" \
        -- -C link-arg=--initial-memory=2097152
done

mkdir -p web/pkg
for module in the_long_slop simulation_worker terrain_worker road_worker grass_worker; do
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
