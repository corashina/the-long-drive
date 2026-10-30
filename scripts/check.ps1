[CmdletBinding(DefaultParameterSetName = 'Routine')]
param(
    [Parameter(Mandatory, ParameterSetName = 'NativeTarget')]
    [ValidatePattern('^[a-z_][a-z0-9_]*$')]
    [string]$Test,

    [Parameter(Mandatory, ParameterSetName = 'NativeFilter')]
    [Parameter(ParameterSetName = 'NativeTarget')]
    [ValidatePattern('^[a-zA-Z_][a-zA-Z0-9_:]*$')]
    [string]$Filter,

    [Parameter(ParameterSetName = 'NativeTarget')]
    [Parameter(ParameterSetName = 'NativeFilter')]
    [switch]$IncludeIgnored,

    [Parameter(Mandatory, ParameterSetName = 'Web')]
    [ValidatePattern('^[a-z0-9]+(?:-[a-z0-9]+)*$')]
    [string[]]$WebTest,

    [Parameter(Mandatory, ParameterSetName = 'Full')]
    [switch]$Full
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
try {
    if ($Full -or $WebTest) {
        $suites = if ($Full) {
            @('loading', 'save-storage', 'save-session', 'save-bridge', 'save-screen', 'autosave', 'world-assets', 'simulation', 'game-mode', 'mirrors', 'net', 'bindings')
        } else { $WebTest }
        $webFiles = @(foreach ($suite in $suites) {
            $file = "$root/tests/web/$suite.test.mjs"
            if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
                throw "Unknown web test suite: $suite"
            }
            $file
        })
        & npm run --silent lint
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        & node --test @webFiles
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        if ($WebTest) { exit 0 }
    }

    $checks = 'cargo fmt --all -- --check'
    if ($Test) {
        $checks += " && cargo test --locked --test $Test"
        if ($Filter) { $checks += " $Filter" }
    } elseif ($Filter) {
        $checks += " && cargo test --locked --workspace --lib $Filter"
    } elseif ($Full) {
        $checks += ' && cargo test --locked --workspace'
    }
    if ($IncludeIgnored) { $checks += ' -- --include-ignored' }
    $checks += ' && cargo check --locked --target wasm32-unknown-unknown --workspace --lib'

    . "$PSScriptRoot/cargo-target.ps1"
    $cargoTarget = Get-CargoTargetDirectory $root
    $image = 'the-long-slop-toolchain'
    & docker build --tag $image --file "$root/tools/Dockerfile" $root
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & docker run --rm --mount "type=bind,source=$root,target=/workspace" `
        --mount 'type=volume,source=tld-wasm-cargo,target=/cargo' `
        --mount 'type=volume,source=tld-wasm-target,target=/target' `
        --env "CARGO_TARGET_DIR=$cargoTarget" $image `
        sh -c $checks
    exit $LASTEXITCODE
} catch {
    Write-Error $_
    exit 1
}
