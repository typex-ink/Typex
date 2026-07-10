[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [ValidateSet("debug", "release")]
    [string]$Profile,
    [string]$CacheDir
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($env:OS -ne "Windows_NT") {
    throw "Windows runtime staging must run on Windows"
}

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$tauriDir = Join-Path $repoRoot "src-tauri"
$stageDir = [IO.Path]::GetFullPath((Join-Path $tauriDir "windows-runtime"))
if (-not $stageDir.StartsWith($tauriDir, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Runtime staging directory escaped src-tauri: $stageDir"
}

if ([string]::IsNullOrWhiteSpace($Profile)) {
    $Profile = if ($env:TAURI_ENV_DEBUG -eq "true") { "debug" } else { "release" }
}

$targetRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    Join-Path $tauriDir "target"
} elseif ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
    $env:CARGO_TARGET_DIR
} else {
    Join-Path $repoRoot $env:CARGO_TARGET_DIR
}
$targetRoot = [IO.Path]::GetFullPath($targetRoot)
$profileDir = Join-Path $targetRoot $Profile

if (-not $SkipBuild) {
    Push-Location $repoRoot
    try {
        $cargoArgs = @(
            "build",
            "--manifest-path", "src-tauri/Cargo.toml",
            "--bin", "typex"
        )
        if ($Profile -eq "release") {
            $cargoArgs += "--release"
        }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Assert-Sha256([string]$Path, [string]$Expected) {
    $actual = Get-Sha256 $Path
    if ($actual -ne $Expected.ToLowerInvariant()) {
        throw "SHA256 mismatch for $Path. Expected $Expected, got $actual"
    }
}

if ([string]::IsNullOrWhiteSpace($CacheDir)) {
    $cacheBase = if (-not [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
        $env:RUNNER_TEMP
    } elseif (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        $env:LOCALAPPDATA
    } else {
        [IO.Path]::GetTempPath()
    }
    $CacheDir = Join-Path $cacheBase "typex-build-cache"
}
$CacheDir = [IO.Path]::GetFullPath($CacheDir)
New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null

function Get-VerifiedDownload(
    [string]$Uri,
    [string]$Destination,
    [string]$Sha256
) {
    if (Test-Path -LiteralPath $Destination) {
        try {
            Assert-Sha256 $Destination $Sha256
            return
        } catch {
            Remove-Item -LiteralPath $Destination -Force
        }
    }

    $partial = "$Destination.partial"
    if (Test-Path -LiteralPath $partial) {
        Remove-Item -LiteralPath $partial -Force
    }
    Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $partial
    Assert-Sha256 $partial $Sha256
    Move-Item -LiteralPath $partial -Destination $Destination -Force
}

function Find-VcRuntimeFiles {
    $redistEnvironmentRoots = New-Object System.Collections.Generic.List[string]
    $matchingToolRoots = New-Object System.Collections.Generic.List[string]
    $fallbackRoots = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrWhiteSpace($env:VCToolsRedistDir)) {
        $redistEnvironmentRoots.Add($env:VCToolsRedistDir)
    }
    if (-not [string]::IsNullOrWhiteSpace($env:VCToolsInstallDir)) {
        $vcTools = (Resolve-Path -LiteralPath $env:VCToolsInstallDir).Path
        $toolVersion = Split-Path $vcTools -Leaf
        $vcRoot = Split-Path (Split-Path (Split-Path $vcTools -Parent) -Parent) -Parent
        $redistRoot = Join-Path $vcRoot "Redist\MSVC"
        $matchingToolRoots.Add((Join-Path $redistRoot $toolVersion))
        $fallbackRoots.Add($redistRoot)
    }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere) {
        $installations = & $vswhere -products * -property installationPath
        foreach ($installation in $installations) {
            if (-not [string]::IsNullOrWhiteSpace($installation)) {
                $fallbackRoots.Add((Join-Path $installation "VC\Redist\MSVC"))
            }
        }
    }

    foreach ($major in @("18", "2022", "17", "2019", "16")) {
        foreach ($edition in @("Community", "Professional", "Enterprise", "BuildTools")) {
            $fallbackRoots.Add((Join-Path $env:ProgramFiles "Microsoft Visual Studio\$major\$edition\VC\Redist\MSVC"))
        }
    }

    function Get-CompleteVcRuntimeSets([string[]]$Roots) {
        $sets = New-Object System.Collections.Generic.List[object]
        $seenDirectories = @{}
        foreach ($root in ($Roots | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Unique)) {
            if (-not (Test-Path -LiteralPath $root)) {
                continue
            }
            $msvcpFiles = Get-ChildItem -LiteralPath $root -Recurse -Filter "msvcp140.dll" -File |
                Where-Object {
                    $_.DirectoryName -match "[\\/]x64[\\/]Microsoft[.]VC[0-9]+[.]CRT$" -and
                    $_.DirectoryName -notmatch "[\\/]onecore[\\/]"
                }
            foreach ($msvcp in $msvcpFiles) {
                $candidate = $msvcp.Directory
                $directoryKey = $candidate.FullName.ToLowerInvariant()
                if ($seenDirectories.ContainsKey($directoryKey)) {
                    continue
                }
                $seenDirectories[$directoryKey] = $true

                $required = @("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll")
                $files = @{}
                foreach ($name in $required) {
                    $path = Join-Path $candidate.FullName $name
                    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                        $files = $null
                        break
                    }
                    $files[$name] = $path
                }
                if ($null -eq $files) {
                    continue
                }

                $runtimeFamily = $candidate.Name -replace "[.]CRT$", ""
                $openMpRuntime = Join-Path (Split-Path $candidate.FullName -Parent) "$runtimeFamily.OpenMP\vcomp140.dll"
                if (-not (Test-Path -LiteralPath $openMpRuntime -PathType Leaf)) {
                    continue
                }
                $files["vcomp140.dll"] = $openMpRuntime

                $versions = @($files.Values | ForEach-Object {
                    $versionText = (Get-Item -LiteralPath $_).VersionInfo.FileVersion
                    $match = [regex]::Match($versionText, "\d+(?:[.]\d+){1,3}")
                    if ($match.Success) { [version]$match.Value }
                })
                $uniqueVersions = @($versions | Select-Object -Unique)
                if ($versions.Count -ne $files.Count -or $uniqueVersions.Count -ne 1) {
                    continue
                }

                $sets.Add([pscustomobject]@{
                    Version = $uniqueVersions[0]
                    Path = $candidate.FullName
                    Files = $files
                }) | Out-Null
            }
        }

        @($sets | Sort-Object -Property @(
            @{ Expression = { $_.Version }; Descending = $true },
            @{ Expression = { $_.Path }; Descending = $true }
        ))
    }

    $redistEnvironmentSets = @(Get-CompleteVcRuntimeSets -Roots @($redistEnvironmentRoots))
    if ($redistEnvironmentSets.Count -gt 0) {
        return $redistEnvironmentSets[0].Files
    }

    $matchingToolSets = @(Get-CompleteVcRuntimeSets -Roots @($matchingToolRoots))
    if ($matchingToolSets.Count -gt 0) {
        return $matchingToolSets[0].Files
    }

    $fallbackSets = @(Get-CompleteVcRuntimeSets -Roots @($fallbackRoots))
    if ($fallbackSets.Count -gt 0) {
        return $fallbackSets[0].Files
    }
    throw "Could not locate the complete x64 Microsoft VC CRT and OpenMP redistributable set"
}

New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
Get-ChildItem -LiteralPath $stageDir -File -ErrorAction SilentlyContinue |
    Remove-Item -Force

$manifestFiles = New-Object System.Collections.Generic.List[object]
function Add-StagedFile(
    [string]$Source,
    [string]$Name,
    [string]$Origin,
    [string]$Version
) {
    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) {
        throw "Required runtime file is missing: $Source"
    }
    $destination = Join-Path $stageDir $Name
    Copy-Item -LiteralPath $Source -Destination $destination -Force
    $manifestFiles.Add([ordered]@{
        name = $Name
        origin = $Origin
        version = $Version
        sha256 = Get-Sha256 $destination
    })
}

$nativeVersions = @{
    "onnxruntime.dll" = "ONNX Runtime 1.17.1"
    "onnxruntime_providers_shared.dll" = "ONNX Runtime 1.17.1"
    "sherpa-onnx-c-api.dll" = "sherpa-rs-sys 0.6.8"
    "sherpa-onnx-cxx-api.dll" = "sherpa-rs-sys 0.6.8"
}
foreach ($name in @(
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll"
)) {
    Add-StagedFile (Join-Path $profileDir $name) $name "cargo-native-build" $nativeVersions[$name]
}

$vcRuntimeFiles = Find-VcRuntimeFiles
foreach ($name in @("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll")) {
    $source = $vcRuntimeFiles[$name]
    $signature = Get-AuthenticodeSignature -LiteralPath $source
    if ($signature.Status -ne "Valid" -or
        $null -eq $signature.SignerCertificate -or
        $signature.SignerCertificate.Subject -notlike "*Microsoft*") {
        throw "Microsoft signature validation failed for ${source}: $($signature.Status)"
    }
    $version = (Get-Item -LiteralPath $source).VersionInfo.FileVersion
    Add-StagedFile $source $name "microsoft-vc-tools-redist-x64" $version
}

$vulkanPackageVersion = "2024.10.25"
$vulkanPackageName = "silk.net.vulkan.loader.native.$vulkanPackageVersion.nupkg"
$vulkanPackage = Join-Path $CacheDir $vulkanPackageName
Get-VerifiedDownload `
    "https://api.nuget.org/v3-flatcontainer/silk.net.vulkan.loader.native/$vulkanPackageVersion/$vulkanPackageName" `
    $vulkanPackage `
    "67a197f94fb22d4a91d7506548a71acdfb05ba1b1b5c28e7ec7bc435067907b9"

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($vulkanPackage)
try {
    $entry = $archive.GetEntry("runtimes/win-x64/native/vulkan-1.dll")
    if ($null -eq $entry) {
        throw "Pinned Vulkan loader package does not contain the win-x64 loader"
    }
    $vulkanDestination = Join-Path $stageDir "vulkan-1.dll"
    $input = $entry.Open()
    $output = [IO.File]::Open($vulkanDestination, [IO.FileMode]::Create, [IO.FileAccess]::Write)
    try {
        $input.CopyTo($output)
    } finally {
        $output.Dispose()
        $input.Dispose()
    }
} finally {
    $archive.Dispose()
}
Assert-Sha256 $vulkanDestination "2cb843cfa9ee9586d2c863ff33454b8ce352a8a96dfe1021b492dfd237ecf8af"
$manifestFiles.Add([ordered]@{
    name = "vulkan-1.dll"
    origin = "NuGet Silk.NET.Vulkan.Loader.Native"
    version = $vulkanPackageVersion
    sha256 = Get-Sha256 $vulkanDestination
})

$notices = @(
    [ordered]@{
        name = "onnxruntime-LICENSE.txt"
        uri = "https://raw.githubusercontent.com/microsoft/onnxruntime/v1.17.1/LICENSE"
        sha256 = "2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c"
        version = "ONNX Runtime 1.17.1"
    },
    [ordered]@{
        name = "onnxruntime-ThirdPartyNotices.txt"
        uri = "https://raw.githubusercontent.com/microsoft/onnxruntime/v1.17.1/ThirdPartyNotices.txt"
        sha256 = "4f9e2bb7b4b407d710a68168615bbc6f70e3d7cc8ba9410fb6c65d92fa71accf"
        version = "ONNX Runtime 1.17.1"
    },
    [ordered]@{
        name = "sherpa-rs-LICENSE.txt"
        uri = "https://raw.githubusercontent.com/thewh1teagle/sherpa-rs/199263d22c32f7a9242d5d9f489f6c4432e0e9f2/LICENSE"
        sha256 = "6abe39d7066d08bf8335a2782b13bd99966498d89bb8fc730fd8900f18b82c9f"
        version = "sherpa-rs-sys 0.6.8"
    },
    [ordered]@{
        name = "vulkan-loader-LICENSE.txt"
        uri = "https://raw.githubusercontent.com/KhronosGroup/Vulkan-Loader/vulkan-sdk-1.3.296.0/LICENSE.txt"
        sha256 = "43c0a37e6a0fa7ff3c843b3ec5a4fac84b712558ddac103fbd4c1649662a9ece"
        version = "Vulkan Loader / Apache-2.0"
    }
)
foreach ($notice in $notices) {
    $cached = Join-Path $CacheDir $notice.name
    Get-VerifiedDownload $notice.uri $cached $notice.sha256
    Add-StagedFile $cached $notice.name "pinned-upstream-license" $notice.version
}

$appVersion = (Get-Content -Raw -LiteralPath (Join-Path $repoRoot "package.json") | ConvertFrom-Json).version
$manifest = [ordered]@{
    schema_version = 1
    target = "x86_64-pc-windows-msvc"
    profile = $Profile
    app_version = $appVersion
    vulkan_loader_package = [ordered]@{
        id = "Silk.NET.Vulkan.Loader.Native"
        version = $vulkanPackageVersion
        sha256 = Get-Sha256 $vulkanPackage
        license = "Apache-2.0"
    }
    files = @($manifestFiles | ForEach-Object { $_ })
}
$manifestPath = Join-Path $stageDir "windows-runtime-manifest.json"
$utf8NoBom = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText(
    $manifestPath,
    (($manifest | ConvertTo-Json -Depth 8) + "`n"),
    $utf8NoBom
)

Write-Host "Staged Windows app-local runtime in $stageDir"
$manifest.files | ForEach-Object {
    Write-Host "  $($_.name) $($_.sha256)"
}
