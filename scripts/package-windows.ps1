param(
    [ValidateSet('debug', 'release')]
    [string] $Profile = 'release',
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string] $Target,
    [string] $OfficeRuntimeDirectory,
    [Parameter(Mandatory = $true)]
    [string] $LicensesDirectory,
    [string[]] $DependencyDirectories = @()
)

# Package existing builds and prepared resources into a fresh portable bundle.
# This does not build, install, restart, publish, or execute packaged binaries.
# Collect licenses with scripts/package/licenses.sh on a supported packaging
# host, then provide its complete output through -LicensesDirectory.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows packaging requires Windows' }
$task_root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$task_host_output = & rustc -vV
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect the Rust toolchain' }
$task_native = ($task_host_output | Where-Object { $_ -match '^host: ' }) -replace '^host: ', ''
if (-not $Target) { $Target = $task_native }
if ($Target -notin @('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')) {
    throw 'A supported Windows MSVC target is required'
}
$task_build = Join-Path $task_root "target/$Target/$Profile"
if (-not (Test-Path -LiteralPath (Join-Path $task_build 'sailry-desktop.exe') -PathType Leaf)) {
    if ($Target -ne $task_native) { throw 'The requested target has not been built' }
    $task_build = Join-Path $task_root "target/$Profile"
}
$task_executable = Join-Path $task_build 'sailry-desktop.exe'
if (-not (Test-Path -LiteralPath $task_executable -PathType Leaf)) {
    throw 'Build sailry-desktop before packaging'
}

function Assert-OrdinaryTree([string] $Path) {
    $task_entries = @((Get-Item -LiteralPath $Path -Force))
    if ($task_entries[0].PSIsContainer) {
        $task_entries += @(Get-ChildItem -LiteralPath $Path -Force -Recurse)
    }
    foreach ($task_entry in $task_entries) {
        if (($task_entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Portable package inputs cannot contain links: $($task_entry.FullName)"
        }
    }
}

function Assert-PeTarget([string] $Path) {
    $task_stream = [IO.File]::OpenRead($Path)
    $task_reader = [IO.BinaryReader]::new($task_stream)
    try {
        if ($task_stream.Length -lt 64 -or $task_reader.ReadUInt16() -ne 0x5a4d) {
            throw "Not a Windows executable: $Path"
        }
        $task_stream.Position = 0x3c
        $task_offset = $task_reader.ReadUInt32()
        if ($task_offset -gt ($task_stream.Length - 6)) { throw "Invalid PE header: $Path" }
        $task_stream.Position = $task_offset
        if ($task_reader.ReadUInt32() -ne 0x00004550) { throw "Invalid PE signature: $Path" }
        $task_expected = if ($Target -eq 'x86_64-pc-windows-msvc') { 0x8664 } else { 0xaa64 }
        if ($task_reader.ReadUInt16() -ne $task_expected) { throw "Wrong executable architecture: $Path" }
    } finally {
        $task_reader.Dispose()
    }
}

Assert-OrdinaryTree $task_executable
Assert-PeTarget $task_executable
if (-not $OfficeRuntimeDirectory) {
    $OfficeRuntimeDirectory = Join-Path $task_root "target/office-runtimes/$Target/office-runtime"
}
$OfficeRuntimeDirectory = (Resolve-Path -LiteralPath $OfficeRuntimeDirectory).ProviderPath
$LicensesDirectory = (Resolve-Path -LiteralPath $LicensesDirectory).ProviderPath
Assert-OrdinaryTree $OfficeRuntimeDirectory
Assert-OrdinaryTree $LicensesDirectory
$task_runtime = Get-Content -LiteralPath (Join-Path $OfficeRuntimeDirectory 'runtime.json') -Raw | ConvertFrom-Json
if ($task_runtime.version -ne 1 -or $task_runtime.target -ne $Target -or
    $task_runtime.executable -ne 'python/python.exe') {
    throw 'The prepared Office runtime does not match the target'
}
Assert-PeTarget (Join-Path $OfficeRuntimeDirectory 'python/python.exe')
foreach ($task_notice in @('dependencies.json', 'source-notices')) {
    if (-not (Test-Path -LiteralPath (Join-Path $LicensesDirectory $task_notice))) {
        throw 'Provide complete license collector output through -LicensesDirectory'
    }
}
$task_hosts = Join-Path $task_root 'target/host-artifacts'
Assert-OrdinaryTree $task_hosts
foreach ($task_linux in @('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu')) {
    foreach ($task_resource in @('sailry-host', 'office-runtime.tar.gz')) {
        if (-not (Test-Path -LiteralPath (Join-Path $task_hosts "$task_linux/$task_resource") -PathType Leaf)) {
            throw 'Prepare bundled Linux hosts with scripts/build-hosts.sh before packaging'
        }
    }
}
$task_manifest = Get-Content -LiteralPath (Join-Path $task_root 'apps/desktop/Cargo.toml') -Raw
$task_version_match = [regex]::Match($task_manifest, '(?m)^version = "([^"]+)"$')
if (-not $task_version_match.Success) { throw 'Cannot read the Desktop application version' }
$task_version = $task_version_match.Groups[1].Value
$task_revision = & git -C $task_root rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot read the source revision' }
$task_changes = & git -C $task_root status --porcelain -- apps crates Cargo.toml Cargo.lock vendor
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect the source state' }
$task_rustc = & rustc --version
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect the Rust compiler' }
$task_dist = Join-Path $task_root 'dist'
[IO.Directory]::CreateDirectory($task_dist) | Out-Null
$task_output = Join-Path $task_dist "windows-$Profile-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $task_output | Out-Null
$task_app = Join-Path $task_output 'Sailry'
New-Item -ItemType Directory -Path $task_app | Out-Null
Copy-Item -LiteralPath $task_executable -Destination (Join-Path $task_app 'sailry-desktop.exe')
$task_libraries = @{}
foreach ($task_directory in @($task_build) + $DependencyDirectories) {
    foreach ($task_library in Get-ChildItem -LiteralPath $task_directory -Filter '*.dll' -File -Force) {
        Assert-OrdinaryTree $task_library.FullName
        Assert-PeTarget $task_library.FullName
        if ($task_libraries.ContainsKey($task_library.Name)) {
            $task_previous = (Get-FileHash -LiteralPath $task_libraries[$task_library.Name] -Algorithm SHA256).Hash
            if ((Get-FileHash -LiteralPath $task_library.FullName -Algorithm SHA256).Hash -ne $task_previous) {
                throw "Conflicting DLL inputs: $($task_library.Name)"
            }
            continue
        }
        Copy-Item -LiteralPath $task_library.FullName -Destination $task_app
        $task_libraries[$task_library.Name] = $task_library.FullName
    }
}
Copy-Item -LiteralPath $OfficeRuntimeDirectory -Destination (Join-Path $task_app 'office-runtime') -Recurse
Copy-Item -LiteralPath $task_hosts -Destination (Join-Path $task_app 'hosts') -Recurse
Copy-Item -LiteralPath $LicensesDirectory -Destination (Join-Path $task_app 'licenses') -Recurse
Copy-Item -LiteralPath (Join-Path $task_root 'Cargo.lock') -Destination $task_app
$task_build_info = [ordered]@{
    version = 1
    source_revision = $task_revision.Trim()
    source_dirty = [bool] $task_changes
    profile = $Profile
    target = $Target
    application_version = $task_version
    rustc = $task_rustc.Trim()
}
$task_utf8 = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $task_app 'build.json'), ($task_build_info | ConvertTo-Json) + "`n", $task_utf8)

# Authenticode publisher signing and signed updater archives are separate release
# operations. This script does neither and never generates signing keys.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$task_archive_name = "Sailry-$task_version-$Target.zip"
$task_archive = Join-Path $task_output $task_archive_name
[IO.Compression.ZipFile]::CreateFromDirectory($task_app, $task_archive, [IO.Compression.CompressionLevel]::Optimal, $true)
$task_digest = (Get-FileHash -LiteralPath $task_archive -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $task_output 'SHA256SUMS'), "$task_digest  $task_archive_name`n", $task_utf8)
Write-Output "Local package: $task_output"
