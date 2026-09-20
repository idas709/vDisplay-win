param([string]$Iscc)
$ErrorActionPreference = 'Stop'
$projectDirectory = Split-Path $PSScriptRoot -Parent
Push-Location $projectDirectory
try {
    if (-not $Iscc) {
        $candidates = @(
            (Join-Path $projectDirectory 'target\tools\inno\ISCC.exe'),
            (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
            (Join-Path $env:ProgramFiles 'Inno Setup 7\ISCC.exe')
        )
        $Iscc = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
        if (-not $Iscc) { $Iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source }
    }
    if (-not $Iscc) { throw 'Install Inno Setup 6.7+ or supply -Iscc <path to ISCC.exe>.' }
    $driver = Join-Path $projectDirectory 'driver\parsec-vdd\parsec-vdd-0.45.0.0.exe'
    $signature = Get-AuthenticodeSignature -LiteralPath $driver
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Parsec Cloud') {
        throw 'The bundled Parsec driver must have a valid Parsec Cloud signature.'
    }
    cargo check
    if ($LASTEXITCODE -ne 0) { throw 'cargo check failed' }
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build --release failed' }
    & $Iscc /Q (Join-Path $PSScriptRoot 'VirtualDisplayWorkspace.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed' }
    $output = Join-Path $projectDirectory 'target\installer\VirtualDisplayWorkspace-Setup.exe'
    $hash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash
    "$hash  VirtualDisplayWorkspace-Setup.exe" | Set-Content -LiteralPath "$output.sha256" -Encoding ascii
    Get-Item -LiteralPath $output | Select-Object FullName, Length
} finally { Pop-Location }
