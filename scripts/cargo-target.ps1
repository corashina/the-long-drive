function Get-CargoTargetDirectory([string]$WorkspaceRoot) {
    $path = [IO.Path]::GetFullPath($WorkspaceRoot).TrimEnd('\', '/').ToLowerInvariant()
    $bytes = [Text.Encoding]::UTF8.GetBytes($path)
    $digest = [Security.Cryptography.SHA256]::HashData($bytes)
    $namespace = [Convert]::ToHexString($digest).ToLowerInvariant()
    return "/target/$namespace"
}
