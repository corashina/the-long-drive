param([switch]$Fast)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot/cargo-target.ps1"
$cargoTarget = Get-CargoTargetDirectory $root
$image = 'the-long-slop-toolchain'
# Fast builds skip LTO, wasm-opt and Brotli. Use them to iterate, not for F3 numbers.
$cargoProfile = if ($Fast) { 'fast' } else { 'release' }
try {
    & docker build --tag $image --file "$root/tools/Dockerfile" $root
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & docker run --rm --mount "type=bind,source=$root,target=/workspace" `
        --mount 'type=volume,source=tld-wasm-cargo,target=/cargo' `
        --mount 'type=volume,source=tld-wasm-target,target=/target' `
        --env "CARGO_TARGET_DIR=$cargoTarget" --env "PROFILE=$cargoProfile" $image `
        sh -c 'cargo build --locked --profile "$PROFILE" --target wasm32-unknown-unknown --lib -p the-long-slop -p simulation-worker -p terrain-worker && cargo rustc --locked --profile "$PROFILE" --target wasm32-unknown-unknown --lib -p road-worker -- -C link-arg=--initial-memory=2097152 && cargo rustc --locked --profile "$PROFILE" --target wasm32-unknown-unknown --lib -p grass-worker -- -C link-arg=--initial-memory=2097152 && mkdir -p web/pkg && wasm-bindgen --target web --out-dir web/pkg --out-name the_long_slop "$CARGO_TARGET_DIR/wasm32-unknown-unknown/$PROFILE/the_long_slop.wasm" && wasm-bindgen --target web --out-dir web/pkg --out-name simulation_worker "$CARGO_TARGET_DIR/wasm32-unknown-unknown/$PROFILE/simulation_worker.wasm" && wasm-bindgen --target web --out-dir web/pkg --out-name terrain_worker "$CARGO_TARGET_DIR/wasm32-unknown-unknown/$PROFILE/terrain_worker.wasm" && wasm-bindgen --target web --out-dir web/pkg --out-name road_worker "$CARGO_TARGET_DIR/wasm32-unknown-unknown/$PROFILE/road_worker.wasm" && wasm-bindgen --target web --out-dir web/pkg --out-name grass_worker "$CARGO_TARGET_DIR/wasm32-unknown-unknown/$PROFILE/grass_worker.wasm" && { [ "$PROFILE" = fast ] || for module in web/pkg/*_bg.wasm; do wasm-opt -O3 --enable-simd --enable-bulk-memory --enable-bulk-memory-opt --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue --enable-call-indirect-overlong "$module" -o "$module" || exit 1; done; }'
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($Fast) { exit 0 }
    & node "$PSScriptRoot/prepare-http-compression.mjs"
    exit $LASTEXITCODE
} catch {
    Write-Error $_
    exit 1
}
