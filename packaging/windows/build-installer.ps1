# Builds the Windows payload, per-user and per-machine MSIs and the self-contained setup EXE.
# Requires WiX v5, WixToolset.BootstrapperApplications.wixext v5, and ImageMagick.
# Usage: build-installer.ps1 [-Out <dir>].
param([string]$Out = ".local/dist/windows-release", [string]$Binaries = "",
      [string]$Root = (Join-Path $PSScriptRoot "../.."))
$ErrorActionPreference = "Stop"
$root = (Resolve-Path -LiteralPath $Root).Path
$release = if ($Binaries) { (Resolve-Path -LiteralPath $Binaries).Path } else { Join-Path $root "target/release" }
$runtimeNames = @(Get-Content -LiteralPath (Join-Path $root "packaging/common/runtime-binaries.txt") | ForEach-Object {
    ($_ -split '\s+')[0] + ".exe"
})
foreach ($name in @($runtimeNames) + @("assetc.exe")) {
    if (-not (Test-Path (Join-Path $release $name))) { throw "missing $release\$name; run make package-binaries" }
}
$manifest = Get-Content -LiteralPath (Join-Path $root "Cargo.toml") -Raw
$workspace = [regex]::Match($manifest, '(?ms)^\[workspace\.package\]\s*\r?\n(.*?)(?=^\[|\z)')
$versionMatch = [regex]::Match($workspace.Groups[1].Value, '(?m)^\s*version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) { throw "missing workspace.package.version in source Cargo.toml" }
$version = $versionMatch.Groups[1].Value

$outPath = [IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Force $outPath | Out-Null
$payload = Join-Path $outPath "payload"
# Only remove this builder's payload directory after resolving it within the output root.
if ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($payload)) -ne $outPath) { throw "unsafe payload path" }
Remove-Item -LiteralPath $payload -Recurse -Force -ErrorAction SilentlyContinue
$resources = Join-Path $payload "resources"
$kit = Join-Path $resources "prep-kit"
New-Item -ItemType Directory -Force (Join-Path $resources "assets"), (Join-Path $resources "licenses"), (Join-Path $resources "fonts") | Out-Null
foreach ($name in $runtimeNames) { Copy-Item (Join-Path $release $name) $payload }
# DX12 compiles shaders with this DXC beside the client; without it the client falls back to FXC.
& (Join-Path $PSScriptRoot "fetch-dxc.ps1") -Root $root -Out $payload
& (Join-Path $PSScriptRoot "stage-prep-kit.ps1") -Root $root -Kit $kit -Assetc (Join-Path $release "assetc.exe")
Copy-Item (Join-Path $root "crates/assets/data/block-physics-v2225.bin") (Join-Path $resources "assets")
Copy-Item (Join-Path $root "THIRD_PARTY_NOTICES.md") (Join-Path $resources "assets")
Copy-Item (Join-Path $root "LICENSE") (Join-Path $resources "licenses/Cinnabar-LICENSE.md")
Copy-Item (Join-Path $root "assets/licenses/*") (Join-Path $resources "licenses")
$fontSource = Get-Content -Raw (Join-Path $root "assets/cinnangles-sans-source.json") | ConvertFrom-Json
$font = Join-Path $root "assets/fonts/$($fontSource.font_file)"
if (-not (Test-Path $font)) { throw "missing $font" }
Copy-Item $font (Join-Path $resources "fonts")

$icon = Join-Path $outPath "cinnabar.ico"
magick -background none -density 384 (Join-Path $PSScriptRoot "../icons/cinnabar.svg") -define icon:auto-resize=256,128,64,48,32,16 $icon
if ($LASTEXITCODE -ne 0) { throw "icon rendering failed" }
$logo = Join-Path $outPath "cinnabar.png"
magick -background none (Join-Path $PSScriptRoot "../icons/cinnabar.svg") -resize 64x64 $logo
if ($LASTEXITCODE -ne 0) { throw "logo rendering failed" }
& (Join-Path $PSScriptRoot "set-executable-icon.ps1") -Executable (Join-Path $payload "cinnabar.exe") -Icon $icon

$signPaths = @($runtimeNames | ForEach-Object { Join-Path $payload $_ }) + @(Join-Path $kit "bin/assetc.exe")
& (Join-Path $PSScriptRoot "execution-level.ps1") -Executable $signPaths -Embed
& (Join-Path $PSScriptRoot "sign.ps1") -Path $signPaths
$install = (Get-Content -Raw (Join-Path $PSScriptRoot "../release-assets.json") | ConvertFrom-Json).windows_install
# The download and update package is per-user; the per-machine one only updates older installs. A
# source whose updater predates per-user packages reads the per-machine key, so it stays per-machine.
$sourceAssets = Get-Content -Raw (Join-Path $root "packaging/release-assets.json") | ConvertFrom-Json
$perUser = $sourceAssets.PSObject.Properties.Name -contains "update_platforms" -and
    $sourceAssets.update_platforms.PSObject.Properties.Name -contains "windows"
$scope = if ($perUser) { "perUser" } else { "perMachine" }
$bundleCode = if ($perUser) { $install.bundle_upgrade_code } else { $install.machine_bundle_upgrade_code }
$msi = Join-Path $outPath "Cinnabar-$version-x64.msi"
$machineMsi = Join-Path $outPath "Cinnabar-$version-x64-machine.msi"
foreach ($package in @(@{ Scope = $scope; Out = $msi }, @{ Scope = "perMachine"; Out = $machineMsi })) {
    wix build -arch x64 -d Version=$version -d Payload=$payload -d Icon=$icon -d "Scope=$($package.Scope)" `
        -d "InstallParent=$($install.parent)" -d "InstallFolder=$($install.folder)" `
        -d "UserKey=$($install.user_key)" -d "UserFolderValue=$($install.user_folder_value)" `
        -d "UserUpgradeCode=$($install.upgrade_code)" -d "MachineUpgradeCode=$($install.machine_upgrade_code)" `
        -o $package.Out (Join-Path $PSScriptRoot "cinnabar.wxs")
    if ($LASTEXITCODE -ne 0) { throw "wix build failed for $($package.Scope)" }
    & (Join-Path $PSScriptRoot "sign.ps1") -Path $package.Out
}
$setup = Join-Path $outPath "Cinnabar-$version-x64-setup.exe"
wix build -arch x64 -ext WixToolset.BootstrapperApplications.wixext -d Version=$version -d Msi=$msi -d Icon=$icon -d Logo=$logo -d "BundleUpgradeCode=$bundleCode" -o $setup (Join-Path $PSScriptRoot "setup.wxs")
if ($LASTEXITCODE -ne 0) { throw "setup bundle build failed" }
& (Join-Path $PSScriptRoot "execution-level.ps1") -Executable $setup
$engine = Join-Path $outPath "setup-engine.exe"
wix burn detach $setup -engine $engine
if ($LASTEXITCODE -ne 0) { throw "setup engine detach failed" }
& (Join-Path $PSScriptRoot "execution-level.ps1") -Executable $engine
if ($env:WINDOWS_CERT_PFX_BASE64) {
    # Burn caches its detached engine; sign that executable as well as the whole bundle.
    & (Join-Path $PSScriptRoot "sign.ps1") -Path $engine
    $signed = Join-Path $outPath "setup-signed.exe"
    wix burn reattach $setup -engine $engine -o $signed
    if ($LASTEXITCODE -ne 0) { throw "setup engine reattach failed" }
    Move-Item -LiteralPath $signed -Destination $setup -Force
}
& (Join-Path $PSScriptRoot "sign.ps1") -Path $setup
Write-Output $msi
Write-Output $machineMsi
Write-Output $setup
