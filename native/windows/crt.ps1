# Stage one coherent app-local CRT from the compiler installed on this runner.
param([string]$Output = ".runtime/native/windows-amd64")

$ErrorActionPreference = 'Stop'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$studio = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if ($LASTEXITCODE -ne 0 -or -not $studio) { throw 'MSVC build tools are unavailable' }
$studio = $studio.Trim()
$version = (Get-Content (Join-Path $studio 'VC\Auxiliary\Build\Microsoft.VCRedistVersion.default.txt') -Raw).Trim()
$redist = Join-Path $studio "VC\Redist\MSVC\$version"
$crt = @(Get-ChildItem (Join-Path $redist 'x64') -Filter 'Microsoft.VC*.CRT' -Directory)
if ($crt.Count -ne 1) { throw 'Expected one x64 Visual C++ CRT redistributable directory' }
$dlls = @(Get-ChildItem $crt[0].FullName -Filter '*.dll' -File | Sort-Object Name)
foreach ($name in 'msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
    if ($name -notin $dlls.Name) { throw "The Visual C++ CRT has no $name" }
}

# Preserve the installed redistribution list and accompanying notices as-is.
# Do not download legal text or substitute an application-authored license.
$notices = @(Get-ChildItem (Join-Path $studio 'Licenses\1033') -File | Sort-Object Name)
if ($notices.Count -eq 0) { throw 'The installed Visual Studio English notices are unavailable' }
New-Item -ItemType Directory -Force $Output | Out-Null
foreach ($dll in $dlls) { Copy-Item -LiteralPath $dll.FullName -Destination (Join-Path $Output $dll.Name) -Force }
foreach ($notice in $notices) { Copy-Item -LiteralPath $notice.FullName -Destination (Join-Path $Output "windows-crt-$($notice.Name)") -Force }

# Include the complete CRT directory: MSVCP also loads its satellite DLLs at
# runtime, so a static import scan alone does not describe its whole closure.
$files = @($dlls.Name) + @($notices | ForEach-Object { "windows-crt-$($_.Name)" })
@{ version = 1; target = 'windows/amd64'; toolset = $version; files = $files } | ConvertTo-Json |
    Set-Content (Join-Path $Output 'windows-crt.json') -Encoding utf8NoBOM
Write-Output "Staged Visual C++ CRT $version ($($crt[0].Name)): $($dlls.Count) DLLs"
