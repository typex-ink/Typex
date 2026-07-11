[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Require-Environment([string]$Name) {
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "$Name is required"
    }
    return $value
}

$assetDir = Require-Environment "ASSET_DIR"
$installerName = Require-Environment "INSTALLER_NAME"
$platformId = if ($env:PLATFORM_ID) { $env:PLATFORM_ID } else { "windows-x86_64" }
$releaseTag = Require-Environment "RELEASE_TAG"
$updaterVersion = Require-Environment "UPDATER_VERSION"
$buildTime = Require-Environment "BUILD_TIME"
$repository = Require-Environment "GITHUB_REPOSITORY"

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "src-tauri/target" }
$bundleDir = Join-Path ([IO.Path]::GetFullPath($targetDir)) "release/bundle/nsis"
if (-not (Test-Path -LiteralPath $bundleDir)) {
    throw "NSIS bundle directory not found: $bundleDir"
}
$sourceInstaller = Get-ChildItem -LiteralPath $bundleDir -Filter "*.exe" -File |
    Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
if (-not $sourceInstaller) { throw "NSIS installer was not produced" }
$sourceSignaturePath = "$($sourceInstaller.FullName).sig"
if (-not (Test-Path -LiteralPath $sourceSignaturePath)) {
    throw "Tauri updater signature was not produced: $sourceSignaturePath"
}

New-Item -ItemType Directory -Force -Path $assetDir | Out-Null
$installerPath = Join-Path $assetDir $installerName
$signaturePath = "$installerPath.sig"
Copy-Item -LiteralPath $sourceInstaller.FullName -Destination $installerPath
Copy-Item -LiteralPath $sourceSignaturePath -Destination $signaturePath

$authenticode = Get-AuthenticodeSignature -LiteralPath $installerPath
$requireAuthenticode = $env:REQUIRE_AUTHENTICODE -eq "true"
if ($requireAuthenticode -and $authenticode.Status -ne "Valid") {
    throw "Authenticode signature is required but status is $($authenticode.Status)"
}
$signature = (Get-Content -Raw -LiteralPath $signaturePath).Trim()
if ([string]::IsNullOrWhiteSpace($signature)) {
    throw "Tauri updater signature is empty"
}
$env:TYPEX_UPDATER_ARTIFACT = $installerPath
$env:TYPEX_UPDATER_SIGNATURE = $signaturePath
try {
    & pnpm verify:updater-signature
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri updater signature verification failed with exit code $LASTEXITCODE"
    }
} finally {
    Remove-Item Env:TYPEX_UPDATER_ARTIFACT -ErrorAction SilentlyContinue
    Remove-Item Env:TYPEX_UPDATER_SIGNATURE -ErrorAction SilentlyContinue
}

$updaterUrl = "https://github.com/$repository/releases/download/$releaseTag/$installerName"
$fragment = [ordered]@{
    version = $updaterVersion
    notes = $env:UPDATE_NOTES
    pub_date = $buildTime
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            url = $updaterUrl
            signature = $signature
        }
    }
}
$utf8NoBom = New-Object Text.UTF8Encoding($false)
$fragmentPath = Join-Path $assetDir "updater-$platformId.json"
[IO.File]::WriteAllText($fragmentPath, (($fragment | ConvertTo-Json -Depth 8) + "`n"), $utf8NoBom)

$metadata = [ordered]@{
    platform = $platformId
    assets = @($installerName, "$installerName.sig")
    authenticode_status = [string]$authenticode.Status
    sha256 = [ordered]@{
        $installerName = (Get-FileHash -Algorithm SHA256 -LiteralPath $installerPath).Hash.ToLowerInvariant()
    }
}
$metadataPath = Join-Path $assetDir "platform-$platformId.json"
[IO.File]::WriteAllText($metadataPath, (($metadata | ConvertTo-Json -Depth 8) + "`n"), $utf8NoBom)
