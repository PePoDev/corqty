$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$destination = Join-Path $root ".env"
if (Test-Path -LiteralPath $destination) {
    Write-Host ".env already exists; preserved. Set CORQTY_MASTER_KEY if upgrading."
    exit 0
}
function New-RandomBytes([int]$Count) {
    $bytes = New-Object byte[] $Count
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    return ,$bytes
}
$text = [System.IO.File]::ReadAllText((Join-Path $root ".env.example"))
foreach ($name in @("postgres_dev", "corqty_owner_dev", "corqty_api_dev", "corqty_worker_dev")) {
    $password = [BitConverter]::ToString((New-RandomBytes 24)).Replace("-", "").ToLowerInvariant()
    $text = $text.Replace($name, $password)
}
$key = [Convert]::ToBase64String((New-RandomBytes 32))
$text = $text.Replace("CORQTY_MASTER_KEY=", "CORQTY_MASTER_KEY=$key")
[System.IO.File]::WriteAllText($destination, $text, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Created .env with fresh local credentials. Back it up securely."
