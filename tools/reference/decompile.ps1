param(
    [Parameter(Mandatory)][string]$AssemblyPath,
    [Parameter(Mandatory)][string]$OutputPath,
    [ValidateSet('Decompile','Index')][string]$Mode = 'Decompile',
    [string]$DecompilerPath = (Join-Path $PSScriptRoot '../../.cache/ilspy/package/lib/netstandard2.0/ICSharpCode.Decompiler.dll')
)
$ErrorActionPreference = 'Stop'
$OutputPath = [IO.Path]::GetFullPath($OutputPath)
$AssemblyPath = [IO.Path]::GetFullPath($AssemblyPath)
[IO.Directory]::CreateDirectory($OutputPath) | Out-Null
function Write-Json($Name, $Value) {
    $json = ConvertTo-Json -InputObject $Value -Depth 100 -Compress
    [IO.File]::WriteAllText((Join-Path $OutputPath $Name), $json + "`n", [Text.UTF8Encoding]::new($false))
}
function Hash-File($Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
# Compile local tools only. Game assemblies enter PEReader and ILSpy as data.
$metadataSources = @((Join-Path $PSScriptRoot 'managed-index.cs'),
                     (Join-Path $PSScriptRoot 'managed-throw-stubs.cs'))
if ($Mode -eq 'Index') {
    Add-Type -Path $metadataSources
    $assemblies = [Collections.Generic.List[object]]::new()
    $types = [Collections.Generic.List[object]]::new()
    $methods = [Collections.Generic.List[object]]::new()
    $calls = [Collections.Generic.List[object]]::new()
    $diagnostics = [Collections.Generic.List[object]]::new()
    $proofs = [Collections.Generic.List[object]]::new()
    foreach ($file in (Get-ChildItem -LiteralPath $AssemblyPath -File -Filter '*.dll' | Sort-Object Name)) {
        $source = 'TheLongDrive_Data/Managed/' + $file.Name
        $item = [ManagedAssetIndex]::Read($file.FullName, $source)
        foreach ($row in $item['types']) { $types.Add($row) }
        foreach ($row in $item['methods']) { $methods.Add($row) }
        foreach ($row in $item['asset_calls']) { $calls.Add($row) }
        foreach ($row in $item['diagnostics']) { $diagnostics.Add($row) }
        foreach ($row in $item['verified_throw_stubs']) { $proofs.Add([ordered]@{assembly=$source;proof=$row}) }
        $assemblies.Add([ordered]@{file=$source;name=$item['name'];version=$item['version'];culture=$item['culture'];
            public_key=$item['public_key'];mvid=$item['mvid'];references=$item['references'];
            bytes=$file.Length;sha256=(Hash-File $file.FullName);type_count=$item['types'].Count;method_count=$item['methods'].Count})
    }
    Write-Json 'index.json' ([ordered]@{schema_version=1;complete=($diagnostics.Count -eq 0);assemblies=$assemblies;
        types=$types;methods=$methods;asset_calls=$calls;diagnostics=$diagnostics;verified_throw_stubs=$proofs;
        metadata_reader=[ordered]@{version=[Reflection.Metadata.MetadataReader].Assembly.GetName().Version.ToString();
            sha256=(Hash-File ([Reflection.Metadata.MetadataReader].Assembly.Location))}})
    if ($diagnostics.Count) { throw 'Managed IL indexing contains errors. See index.json.' }
    exit 0
}
$stem = [IO.Path]::GetFileNameWithoutExtension($AssemblyPath)
$sourceFile = 'TheLongDrive_Data/Managed/' + [IO.Path]::GetFileName($AssemblyPath)
$receiptName = if ($stem -eq 'Assembly-CSharp') { 'decompilation.json' } else { $stem + '.decompilation.json' }
$receipt = [ordered]@{schema_version=1;complete=$false;mode='whole_module';
    input=[ordered]@{file=$sourceFile;sha256=$null};
    host=[ordered]@{powershell=$PSVersionTable.PSVersion.ToString();dotnet=[Environment]::Version.ToString()};
    diagnostics=@()}
try {
    $inputHash = Hash-File $AssemblyPath
    $receipt['input']['sha256'] = $inputHash
    $DecompilerPath = [IO.Path]::GetFullPath($DecompilerPath)
    $receipt['decompiler'] = [ordered]@{name='ICSharpCode.Decompiler';sha256=(Hash-File $DecompilerPath)}
    $packageRoot = [IO.Directory]::GetParent([IO.Directory]::GetParent([IO.Path]::GetDirectoryName($DecompilerPath)).FullName).FullName
    $nuspec = Join-Path $packageRoot 'ICSharpCode.Decompiler.nuspec'
    $receipt['decompiler']['package_sha256'] = Hash-File $nuspec
    [xml]$package = [IO.File]::ReadAllText($nuspec)
    $receipt['decompiler']['package_version'] = [string]$package.package.metadata.version
    if ($package.package.metadata.version -ne '11.1.0.9782') { throw 'Decompiler package version mismatch.' }
    $decompilerAssembly = [Reflection.Assembly]::LoadFrom($DecompilerPath)
    $receipt['decompiler']['version'] = $decompilerAssembly.GetName().Version.ToString()
    if ($decompilerAssembly.GetName().Version.ToString() -ne '11.1.0.9782') { throw 'Decompiler assembly version mismatch.' }
    $dependencies = [Collections.Generic.List[object]]::new()
    $receipt['decompiler']['dependencies'] = $dependencies
    foreach ($dependency in $package.package.metadata.dependencies.group.dependency) {
        $loaded = [Reflection.Assembly]::Load([string]$dependency.id)
        $required = [Version]([string]$dependency.version)
        $dependencies.Add([ordered]@{name=[string]$dependency.id;required=[string]$dependency.version;
            version=$loaded.GetName().Version.ToString();sha256=(Hash-File $loaded.Location)})
        if ($loaded.GetName().Version -lt $required) { throw ('Decompiler dependency version mismatch: ' + $dependency.id) }
    }
    $references = @((Get-ChildItem "$PSHOME/ref/*.dll").FullName) + @($DecompilerPath)
    Add-Type -CompilerOptions /nowarn:1701 -ReferencedAssemblies $references -Path ($metadataSources + @((Join-Path $PSScriptRoot 'managed-coverage.cs')))
    $result = [ManagedMethodCoverage]::Export($AssemblyPath, $OutputPath, $stem)
    $codePath = Join-Path $OutputPath ($stem + '.cs')
    $receipt['output'] = [ordered]@{file=('managed/' + $stem + '.cs');
        bytes=(Get-Item -LiteralPath $codePath).Length;sha256=(Hash-File $codePath)}
    $coverageName = $stem + '.coverage.json'
    Write-Json $coverageName ([ordered]@{schema_version=1;input=$receipt['input'];methods=$result['methods']})
    $coveragePath = Join-Path $OutputPath $coverageName
    $receipt['coverage'] = [ordered]@{file=('managed/' + $coverageName);
        bytes=(Get-Item -LiteralPath $coveragePath).Length;sha256=(Hash-File $coveragePath)}
    $receipt['supplement'] = $null
    if ($result['supplement']) {
        $supplementPath = Join-Path $OutputPath ([IO.Path]::GetFileName($result['supplement']))
        $receipt['supplement'] = [ordered]@{file=$result['supplement'];
            bytes=(Get-Item -LiteralPath $supplementPath).Length;sha256=(Hash-File $supplementPath)}
    }
    $diagnostics = @($result['diagnostics'])
    $receipt['diagnostics'] = $diagnostics
    $errors = @($diagnostics | Where-Object { $_ -match 'Error decompiling|ILSpy could not decompile' })
    $metadata = [ManagedAssetIndex]::Read($AssemblyPath, $sourceFile)
    $receipt['type_count'] = $metadata['types'].Count
    $receipt['method_count'] = $metadata['methods'].Count
    $complete = ($errors.Count -eq 0 -and $metadata['diagnostics'].Count -eq 0)
    $receipt['complete'] = $complete
    Write-Json $receiptName $receipt
    if ((Hash-File $AssemblyPath) -ne $inputHash) { throw 'Decompilation input changed.' }
    if (-not $complete) { throw ('Decompiler reported incomplete code. See ' + $receiptName) }
} catch {
    $receipt['complete'] = $false
    $receipt['diagnostics'] = @($receipt['diagnostics']) + @($_.Exception.ToString())
    Write-Json $receiptName $receipt
    throw
}
