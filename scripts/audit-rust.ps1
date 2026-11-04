$ErrorActionPreference = "Stop"
# SQLx's lockfile includes optional MySQL/RSA packages even with only PostgreSQL
# enabled. RUSTSEC-2023-0071 has no fix. The exception is valid only while no
# workspace build on any target includes RSA; fail before auditing if that changes.
$dependencies = cargo tree --workspace --target all --prefix none
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($dependencies | Select-String '^rsa v') {
    throw "RSA is now compiled: remove the lockfile-only advisory exception and remediate it."
}
cargo audit --ignore RUSTSEC-2023-0071 --deny warnings
exit $LASTEXITCODE
