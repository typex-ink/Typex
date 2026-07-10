[CmdletBinding()]
param(
    [switch]$RequireInstallerExtraction,
    [switch]$RequireUpdaterSignature,
    [ValidateSet("debug", "release")]
    [string]$Profile = "release"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($env:OS -ne "Windows_NT") {
    throw "Windows bundle verification must run on Windows"
}

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$tauriDir = Join-Path $repoRoot "src-tauri"
$stageDir = Join-Path $tauriDir "windows-runtime"
$targetRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    Join-Path $tauriDir "target"
} elseif ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
    $env:CARGO_TARGET_DIR
} else {
    Join-Path $repoRoot $env:CARGO_TARGET_DIR
}
$targetRoot = [IO.Path]::GetFullPath($targetRoot)
$profileDir = Join-Path $targetRoot $Profile
$bundleDir = Join-Path $profileDir "bundle\nsis"
$nsisWorkDir = Join-Path $profileDir "nsis\x64"
$manifestPath = Join-Path $stageDir "windows-runtime-manifest.json"
$binaryPath = Join-Path $profileDir "typex.exe"
$tempBase = if (-not [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    $env:RUNNER_TEMP
} else {
    [IO.Path]::GetTempPath()
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Assert-Exists([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required file is missing: $Path"
    }
}

Assert-Exists $manifestPath
Assert-Exists $binaryPath
if (-not (Test-Path -LiteralPath $bundleDir -PathType Container)) {
    throw "NSIS bundle directory is missing: $bundleDir"
}

$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
if ($manifest.schema_version -ne 1 -or $manifest.target -ne "x86_64-pc-windows-msvc") {
    throw "Unexpected Windows runtime manifest schema or target"
}
$appVersion = (Get-Content -Raw -LiteralPath (Join-Path $repoRoot "package.json") | ConvertFrom-Json).version
if ($manifest.profile -ne $Profile -or $manifest.app_version -ne $appVersion) {
    throw "Windows runtime manifest profile or app version does not match the bundle"
}

$requiredRuntime = @(
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll",
    "msvcp140.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "vcomp140.dll",
    "vulkan-1.dll"
)
$manifestByName = @{}
foreach ($file in $manifest.files) {
    if ($manifestByName.ContainsKey($file.name)) {
        throw "Duplicate runtime manifest entry: $($file.name)"
    }
    $path = Join-Path $stageDir $file.name
    Assert-Exists $path
    $actual = Get-Sha256 $path
    if ($actual -ne $file.sha256) {
        throw "Staged runtime hash mismatch for $($file.name)"
    }
    $manifestByName[$file.name.ToLowerInvariant()] = $path
}
foreach ($name in $requiredRuntime) {
    if (-not $manifestByName.ContainsKey($name)) {
        throw "Runtime manifest is missing $name"
    }
}

$dependencyTool = if (Get-Command dumpbin.exe -ErrorAction SilentlyContinue) {
    "dumpbin"
} elseif (Get-Command llvm-objdump.exe -ErrorAction SilentlyContinue) {
    "llvm-objdump"
} elseif (Test-Path -LiteralPath "C:\Program Files\LLVM\bin\llvm-objdump.exe") {
    "C:\Program Files\LLVM\bin\llvm-objdump.exe"
} else {
    throw "dumpbin.exe or llvm-objdump.exe is required for PE dependency verification"
}

function Get-PeImports([string]$Path) {
    if ($dependencyTool -eq "dumpbin") {
        $output = & dumpbin.exe /nologo /dependents $Path 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "dumpbin failed for $Path"
        }
        return @($output | ForEach-Object {
            if ($_ -match "^\s+([A-Za-z0-9_.-]+[.]dll)\s*$") { $Matches[1] }
        } | Where-Object { $_ } | Sort-Object -Unique)
    }

    $output = & $dependencyTool -p $Path 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "llvm-objdump failed for $Path"
    }
    return @($output | ForEach-Object {
        if ($_ -match "DLL Name:\s*([^\s]+[.]dll)") { $Matches[1] }
    } | Where-Object { $_ } | Sort-Object -Unique)
}

function Assert-Imports([string]$Path, [string[]]$Required) {
    $imports = @(Get-PeImports $Path)
    foreach ($name in $Required) {
        if ($imports -notcontains $name) {
            throw "$([IO.Path]::GetFileName($Path)) does not import required dependency $name"
        }
    }
}

function Test-WindowsSystemImport([string]$Name) {
    if ($Name -match "^(api-ms-win-|ext-ms-win-)") {
        return $true
    }
    if ($Name -match "^(msvcp|msvcr|vcruntime|vcomp|concrt|mfc|atl)[a-z0-9_.-]*[.]dll$" -or
        $Name -ieq "ucrtbased.dll") {
        return $false
    }
    $systemPath = Join-Path $env:SystemRoot "System32\$Name"
    if (-not (Test-Path -LiteralPath $systemPath -PathType Leaf)) {
        return $false
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $systemPath
    return $signature.Status -eq "Valid" -and
        $null -ne $signature.SignerCertificate -and
        $signature.SignerCertificate.Subject -like "*Microsoft*"
}

function Assert-PayloadDependencyClosure(
    [string]$Executable,
    [hashtable]$RuntimeByName
) {
    Assert-Imports $Executable @("sherpa-onnx-c-api.dll", "vulkan-1.dll", "VCOMP140.dll")
    Assert-Imports $RuntimeByName["sherpa-onnx-c-api.dll"] @("onnxruntime.dll")
    Assert-Imports $RuntimeByName["sherpa-onnx-cxx-api.dll"] @("sherpa-onnx-c-api.dll")
    Assert-Imports $RuntimeByName["onnxruntime.dll"] @(
        "MSVCP140.dll",
        "VCRUNTIME140.dll",
        "VCRUNTIME140_1.dll"
    )

    $pePayloads = @($Executable) + @($RuntimeByName.Values)
    foreach ($payload in $pePayloads) {
        foreach ($import in (Get-PeImports $payload)) {
            $key = $import.ToLowerInvariant()
            if ($RuntimeByName.ContainsKey($key) -or (Test-WindowsSystemImport $import)) {
                continue
            }
            throw "$([IO.Path]::GetFileName($payload)) has unresolved non-system import $import"
        }
    }
}

$stagedRuntimeByName = @{}
foreach ($name in $requiredRuntime) {
    $stagedRuntimeByName[$name.ToLowerInvariant()] = Join-Path $stageDir $name
}
Assert-PayloadDependencyClosure $binaryPath $stagedRuntimeByName

$expectedInstalled = @{
    "windows-runtime-manifest.json" = $manifestPath
    "THIRD-PARTY-NOTICES.windows.txt" = Join-Path $tauriDir "windows-runtime-notices\THIRD-PARTY-NOTICES.windows.txt"
    "Apache-2.0.txt" = Join-Path $tauriDir "vendor\sherpa-rs-sys\sherpa-onnx\LICENSE"
}
foreach ($file in $manifest.files) {
    $expectedInstalled[$file.name] = Join-Path $stageDir $file.name
}
foreach ($source in $expectedInstalled.Values) {
    Assert-Exists $source
}

$installerScript = Join-Path $nsisWorkDir "installer.nsi"
Assert-Exists $installerScript
$installerScriptText = Get-Content -Raw -LiteralPath $installerScript
foreach ($name in @("typex.exe") + @($expectedInstalled.Keys)) {
    if (-not $installerScriptText.Contains($name)) {
        throw "Rendered NSIS script does not include $name"
    }
}

function Find-SevenZip {
    $command = Get-Command 7z.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $programFilesX86SevenZip = if (${env:ProgramFiles(x86)}) {
        Join-Path ${env:ProgramFiles(x86)} "7-Zip\7z.exe"
    } else {
        $null
    }
    $chocolateySevenZip = if ($env:ChocolateyInstall) {
        Join-Path $env:ChocolateyInstall "bin\7z.exe"
    } else {
        $null
    }
    foreach ($candidate in @(
        (Join-Path $env:ProgramFiles "7-Zip\7z.exe"),
        $programFilesX86SevenZip,
        $chocolateySevenZip
    )) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            return $candidate
        }
    }
    return $null
}

$sevenZip = Find-SevenZip
if (-not $sevenZip -and $RequireInstallerExtraction) {
    throw "Full 7-Zip is required to extract and verify the NSIS installer"
}

function Assert-ExtractedPayload(
    [string]$Installer,
    [string]$Label
) {
    if (-not $sevenZip) {
        Write-Warning "Skipping NSIS extraction for $Label because full 7-Zip was not found"
        return
    }
    $extractRoot = Join-Path $tempBase ("typex-nsis-audit-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $extractRoot | Out-Null
    & $sevenZip x -y "-o$extractRoot" $Installer | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip failed to extract $Label"
    }

    $extractedByName = @{}
    foreach ($entry in $expectedInstalled.GetEnumerator()) {
        $expectedHash = Get-Sha256 $entry.Value
        $matches = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -File -Filter $entry.Key)
        if ($matches.Count -ne 1) {
            throw "$Label must contain exactly one $($entry.Key); found $($matches.Count)"
        }
        if ((Get-Sha256 $matches[0].FullName) -ne $expectedHash) {
            throw "$Label contains $($entry.Key), but its hash does not match the staged payload"
        }
        $extractedByName[$entry.Key.ToLowerInvariant()] = $matches[0].FullName
    }

    $executableMatches = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -File -Filter "typex.exe")
    if ($executableMatches.Count -ne 1) {
        throw "$Label must contain exactly one typex.exe; found $($executableMatches.Count)"
    }

    $expectedPayloadLocations = @{
        "windows-runtime-manifest.json" = "windows-runtime-manifest.json"
        "THIRD-PARTY-NOTICES.windows.txt" = "licenses\THIRD-PARTY-NOTICES.windows.txt"
        "Apache-2.0.txt" = "licenses\Apache-2.0.txt"
    }
    foreach ($file in $manifest.files) {
        $relativePath = if ([IO.Path]::GetExtension($file.name) -eq ".txt") {
            "licenses\$($file.name)"
        } else {
            $file.name
        }
        $expectedPayloadLocations[$file.name] = $relativePath
    }
    foreach ($entry in $expectedPayloadLocations.GetEnumerator()) {
        $actualPath = [IO.Path]::GetFullPath($extractedByName[$entry.Key.ToLowerInvariant()])
        $expectedPath = [IO.Path]::GetFullPath((Join-Path $extractRoot $entry.Value))
        if (-not $actualPath.Equals($expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
            throw "$Label places $($entry.Key) outside its required location $($entry.Value)"
        }
    }

    $webviewBootstrappers = @(
        Get-ChildItem -LiteralPath $extractRoot -Recurse -File -Filter "MicrosoftEdgeWebview2Setup.exe"
    )
    if ($webviewBootstrappers.Count -ne 1) {
        throw "$Label must contain exactly one WebView2 Evergreen Bootstrapper"
    }
    $webviewSignature = Get-AuthenticodeSignature -LiteralPath $webviewBootstrappers[0].FullName
    if ($webviewSignature.Status -ne "Valid" -or
        $null -eq $webviewSignature.SignerCertificate -or
        $webviewSignature.SignerCertificate.Subject -notlike "*Microsoft*") {
        throw "$Label contains a WebView2 Bootstrapper without a valid Microsoft signature"
    }

    $extractedExecutable = $executableMatches[0].FullName
    $expectedVersion = (Get-Item -LiteralPath $binaryPath).VersionInfo
    $actualVersion = (Get-Item -LiteralPath $extractedExecutable).VersionInfo
    foreach ($property in @("ProductName", "ProductVersion", "OriginalFilename")) {
        if ($actualVersion.$property -ne $expectedVersion.$property) {
            throw "$Label typex.exe has unexpected $property metadata"
        }
    }

    $expectedImports = @(Get-PeImports $binaryPath)
    $actualImports = @(Get-PeImports $extractedExecutable)
    $importDifference = @(Compare-Object $expectedImports $actualImports)
    if ($importDifference.Count -ne 0) {
        throw "$Label typex.exe imports differ from the release build"
    }

    $extractedRuntimeByName = @{}
    foreach ($name in $requiredRuntime) {
        $extractedRuntimeByName[$name.ToLowerInvariant()] = $extractedByName[$name.ToLowerInvariant()]
    }
    Assert-PayloadDependencyClosure $extractedExecutable $extractedRuntimeByName
}

$installer = Get-ChildItem -LiteralPath $bundleDir -Filter "*.exe" -File |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1
if (-not $installer) {
    throw "NSIS installer was not produced"
}
Assert-ExtractedPayload $installer.FullName "NSIS installer $($installer.Name)"

$updaterSignaturePath = "$($installer.FullName).sig"
if ($RequireUpdaterSignature -and -not (Test-Path -LiteralPath $updaterSignaturePath -PathType Leaf)) {
    throw "Tauri updater signature was not produced: $updaterSignaturePath"
}
if (Test-Path -LiteralPath $updaterSignaturePath -PathType Leaf) {
    $signature = (Get-Content -Raw -LiteralPath $updaterSignaturePath).Trim()
    if ([string]::IsNullOrWhiteSpace($signature)) {
        throw "Tauri updater signature is empty"
    }
    if ([string]::IsNullOrWhiteSpace($env:TAURI_UPDATER_PUBKEY)) {
        if ($RequireUpdaterSignature) {
            throw "TAURI_UPDATER_PUBKEY is required to verify the updater signature"
        }
        Write-Warning "Skipping updater signature verification because TAURI_UPDATER_PUBKEY is not set"
    } else {
        $env:TYPEX_UPDATER_ARTIFACT = $installer.FullName
        $env:TYPEX_UPDATER_SIGNATURE = $updaterSignaturePath
        try {
            & pnpm verify:updater-signature
            if ($LASTEXITCODE -ne 0) {
                throw "Tauri updater signature verification failed with exit code $LASTEXITCODE"
            }
        } finally {
            Remove-Item Env:TYPEX_UPDATER_ARTIFACT -ErrorAction SilentlyContinue
            Remove-Item Env:TYPEX_UPDATER_SIGNATURE -ErrorAction SilentlyContinue
        }
    }
}

Write-Host "Windows NSIS payload, PE dependency closure, and available updater signature verified"
