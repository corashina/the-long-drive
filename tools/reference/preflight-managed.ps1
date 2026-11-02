param(
    [string]$PackagePath = (Join-Path $PSScriptRoot '../../.cache/ilspy/package')
)
$ErrorActionPreference = 'Stop'
# Read only tool metadata. Do not compile helpers or read game assemblies.
$dll = Join-Path $PackagePath 'lib/netstandard2.0/ICSharpCode.Decompiler.dll'
$nuspec = Join-Path $PackagePath 'ICSharpCode.Decompiler.nuspec'
[xml]$package = [IO.File]::ReadAllText([IO.Path]::GetFullPath($nuspec))
if ($package.package.metadata.version -ne '11.1.0.9782') { throw 'Decompiler package version mismatch.' }
$assembly = [Reflection.Assembly]::LoadFrom([IO.Path]::GetFullPath($dll))
if ($assembly.GetName().Version.ToString() -ne '11.1.0.9782') { throw 'Decompiler assembly version mismatch.' }
foreach ($dependency in $package.package.metadata.dependencies.group.dependency) {
    $loaded = [Reflection.Assembly]::Load([string]$dependency.id)
    if ($loaded.GetName().Version -lt [Version]([string]$dependency.version)) {
        throw ('Decompiler dependency version mismatch: ' + $dependency.id)
    }
}
Write-Output ('Managed tools ready: PowerShell ' + $PSVersionTable.PSVersion + '; .NET ' + [Environment]::Version)
