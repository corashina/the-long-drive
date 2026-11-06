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
        sh tools/build.sh
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($Fast) { exit 0 }
    & node "$PSScriptRoot/prepare-http-compression.mjs"
    exit $LASTEXITCODE
} catch {
    Write-Error $_
    exit 1
}
