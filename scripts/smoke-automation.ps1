param(
    [string]$ApiBase = "http://localhost:8080/api/v1",
    [string]$WebBase = "http://localhost:3000"
)
$ErrorActionPreference = "Stop"
$ApiBase = $ApiBase.TrimEnd('/')
$origin = $WebBase.TrimEnd('/')
$session = New-Object Microsoft.PowerShell.Commands.WebRequestSession
$csrf = $null

function Invoke-AutomationRequest {
    param([string]$Method, [string]$Path, [object]$Payload, [hashtable]$Headers = @{}, [int]$Expected = 200, [switch]$Anonymous, [string]$RawBody)
    $requestHeaders = @{}
    foreach ($key in $Headers.Keys) { $requestHeaders[$key] = $Headers[$key] }
    if (-not $Anonymous -and $csrf) { $requestHeaders['x-csrf-token'] = $csrf }
    $uri = if ($Path.StartsWith('/hooks/')) { "$origin$Path" } else { "$ApiBase$Path" }
    $request = @{ UseBasicParsing = $true; Method = $Method; Uri = $uri; Headers = $requestHeaders }
    if (-not $Anonymous) { $request.WebSession = $session }
    if ($null -ne $Payload -or $PSBoundParameters.ContainsKey('RawBody')) {
        $request.ContentType = 'application/json'
        $json = if ($PSBoundParameters.ContainsKey('RawBody')) { $RawBody } else { ConvertTo-Json -InputObject $Payload -Depth 32 -Compress }
        $request.Body = [Text.Encoding]::UTF8.GetBytes($json)
    }
    try {
        $response = Invoke-WebRequest @request
        $status = [int]$response.StatusCode
        $content = $response.Content
    } catch {
        if ($null -eq $_.Exception.Response) { throw }
        $status = [int]$_.Exception.Response.StatusCode
        $content = $_.ErrorDetails.Message
    }
    if ($status -ne $Expected) { throw "$Method $Path returned $status, expected $Expected" }
    if ($content) { return ($content | ConvertFrom-Json) }
}

$suffix = [guid]::NewGuid().ToString('N')
$null = Invoke-AutomationRequest -Method Post -Path '/auth/register' -Expected 201 -Payload @{
    email = "automation-$suffix@example.com"; password = 'correct horse battery staple'; organizationName = 'Automation smoke'
}
$csrf = $session.Cookies.GetCookies([uri]$ApiBase) | Where-Object { $_.Name -eq 'corqty_csrf' } | Select-Object -ExpandProperty Value
if (-not $csrf) { throw 'Registration did not issue a CSRF cookie' }
$me = Invoke-AutomationRequest -Method Get -Path '/auth/me'
$org = "/orgs/$($me.organizations[0].organizationId)"
$workflow = Invoke-AutomationRequest -Method Post -Path "$org/workflows" -Payload @{ name = 'Automation smoke' } -Expected 201
$workflowPath = "$org/workflows/$($workflow.id)"
$null = Invoke-AutomationRequest -Method Post -Path "$workflowPath/publish" -Payload @{ revision = 1 } -Expected 201

$token = Invoke-AutomationRequest -Method Post -Path "$org/service-tokens" -Payload @{ name = 'Smoke client' } -Expected 201
$headers = @{ Authorization = "Bearer $($token.token)"; 'Idempotency-Key' = "smoke-$suffix" }
$first = Invoke-AutomationRequest -Method Post -Path "$workflowPath/runs" -Payload @{} -Headers $headers -Anonymous -Expected 201
$again = Invoke-AutomationRequest -Method Post -Path "$workflowPath/runs" -Payload @{} -Headers $headers -Anonymous
if ($first.id -ne $again.id -or $first.triggerSource -ne 'api') { throw 'Service-token run was not idempotent' }
$null = Invoke-AutomationRequest -Method Post -Path "$workflowPath/runs" -Payload @{ input = @{ changed = $true } } -Headers $headers -Anonymous -Expected 409
$null = Invoke-AutomationRequest -Method Delete -Path "$org/service-tokens/$($token.id)" -Expected 204
$null = Invoke-AutomationRequest -Method Post -Path "$workflowPath/runs" -Payload @{} -Headers $headers -Anonymous -Expected 401

$hook = Invoke-AutomationRequest -Method Post -Path "$org/triggers" -Payload @{
    name = 'Inbound smoke'; workflowId = $workflow.id; version = 1; kind = 'webhook'
} -Expected 201
$timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
$rawBody = '{}'
$hmac = New-Object System.Security.Cryptography.HMACSHA256
try {
    $hmac.Key = [Text.Encoding]::UTF8.GetBytes($hook.webhookSecret)
    $signature = [BitConverter]::ToString($hmac.ComputeHash([Text.Encoding]::UTF8.GetBytes("$timestamp.$rawBody"))).Replace('-', '').ToLowerInvariant()
} finally { $hmac.Dispose() }
$hookHeaders = @{ 'X-Corqty-Timestamp' = $timestamp; 'X-Corqty-Signature' = $signature; 'Idempotency-Key' = "hook-$suffix" }
$firstHook = Invoke-AutomationRequest -Method Post -Path $hook.webhookPath -RawBody $rawBody -Headers $hookHeaders -Anonymous -Expected 201
$againHook = Invoke-AutomationRequest -Method Post -Path $hook.webhookPath -RawBody $rawBody -Headers $hookHeaders -Anonymous
if ($firstHook.id -ne $againHook.id -or $firstHook.triggerSource -ne 'webhook') { throw 'Webhook run was not idempotent' }
$null = Invoke-AutomationRequest -Method Post -Path $hook.webhookPath -RawBody '{"tampered":true}' -Headers $hookHeaders -Anonymous -Expected 401
$listed = Invoke-AutomationRequest -Method Get -Path "$org/triggers"
if (($listed | ConvertTo-Json -Depth 32 -Compress).Contains($hook.webhookSecret)) { throw 'Webhook secret appeared in a read response' }

$secretValue = "smoke-secret-$suffix"
$secret = Invoke-AutomationRequest -Method Post -Path "$org/secrets" -Payload @{ name = 'Smoke secret'; value = $secretValue } -Expected 201
$listedSecrets = Invoke-AutomationRequest -Method Get -Path "$org/secrets"
if (($listedSecrets | ConvertTo-Json -Depth 32 -Compress).Contains($secretValue)) { throw 'Secret plaintext appeared in a read response' }
$null = Invoke-AutomationRequest -Method Delete -Path "$org/secrets/$($secret.id)" -Expected 204

$cron = Invoke-AutomationRequest -Method Post -Path "$org/triggers" -Payload @{
    name = 'Minute smoke'; workflowId = $workflow.id; version = 1; kind = 'cron'; cron = '* * * * *'; timezone = 'UTC'
} -Expected 201
$deadline = [DateTimeOffset]::UtcNow.AddSeconds(75)
$observed = $false
while ([DateTimeOffset]::UtcNow -lt $deadline) {
    $runs = Invoke-AutomationRequest -Method Get -Path "$workflowPath/runs"
    if (@($runs.items | Where-Object { $_.triggerSource -eq 'cron' -and $_.version -eq 1 }).Count -gt 0) { $observed = $true; break }
    Start-Sleep -Seconds 2
}
if (-not $observed) { throw 'Cron did not create a pinned run within 75 seconds' }
$null = Invoke-AutomationRequest -Method Delete -Path "$org/triggers/$($cron.id)" -Expected 204
$null = Invoke-AutomationRequest -Method Delete -Path "$org/triggers/$($hook.id)" -Expected 204
Write-Host 'Automation smoke passed: service tokens, idempotency, signed webhooks, secret metadata, and cron.'
