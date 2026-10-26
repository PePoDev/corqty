param(
    [string]$ProjectName = "corqty",
    [string]$ApiBase = "http://localhost:8080/api/v1",
    [string]$WebBase = "http://localhost:3000",
    [int]$ExpectedWorkers = 1
)

$ErrorActionPreference = "Stop"

$health = Invoke-WebRequest -UseBasicParsing "$ApiBase/health"
if ($health.StatusCode -ne 204) {
    throw "API health check returned $($health.StatusCode)"
}

$web = Invoke-WebRequest -UseBasicParsing $WebBase
if ($web.StatusCode -ne 200 -or $web.Content -notmatch "<title>Corqty</title>") {
    throw "Web application did not serve the Corqty shell"
}

function Register-SmokeUser {
    param(
        [string]$Email,
        [string]$OrganizationName
    )

    $webSession = New-Object Microsoft.PowerShell.Commands.WebRequestSession
    $body = @{
        email = $Email
        password = "correct horse battery staple"
        organizationName = $OrganizationName
    } | ConvertTo-Json
    $registration = Invoke-WebRequest `
        -UseBasicParsing `
        -WebSession $webSession `
        -Method Post `
        -ContentType "application/json" `
        -Body $body `
        "$ApiBase/auth/register"
    if ($registration.StatusCode -ne 201) {
        throw "Registration returned $($registration.StatusCode)"
    }
    $csrf = $webSession.Cookies.GetCookies([uri]$ApiBase) |
        Where-Object { $_.Name -eq "corqty_csrf" } |
        Select-Object -ExpandProperty Value
    if (-not $csrf) {
        throw "Registration did not set the CSRF cookie"
    }
    [pscustomobject]@{
        Session = $webSession
        Csrf = $csrf
    }
}

# Returns the status and the parsed JSON body, for problems as well as successes.
function Invoke-SmokeRequest {
    param(
        [pscustomobject]$User,
        [string]$Method,
        [string]$Path,
        [hashtable]$Body
    )

    $request = @{
        UseBasicParsing = $true
        WebSession = $User.Session
        Method = $Method
        Uri = "$ApiBase$Path"
    }
    if ($Body) {
        $request.ContentType = "application/json"
        $request.Headers = @{ "x-csrf-token" = $User.Csrf }
        $request.Body = ConvertTo-Json -InputObject $Body -Depth 32 -Compress
    }
    try {
        $response = Invoke-WebRequest @request
        $status = [int]$response.StatusCode
        $content = $response.Content
    } catch {
        if ($null -eq $_.Exception.Response) {
            throw
        }
        $status = [int]$_.Exception.Response.StatusCode
        $content = $_.ErrorDetails.Message
    }
    [pscustomobject]@{
        Status = $status
        Body = if ($content) { $content | ConvertFrom-Json } else { $null }
    }
}

$suffix = [guid]::NewGuid().ToString("N")
$admin = Register-SmokeUser `
    -Email "admin-$suffix@example.com" `
    -OrganizationName "Smoke Organization"
$adminMe = Invoke-RestMethod `
    -WebSession $admin.Session `
    "$ApiBase/auth/me"
if (
    $adminMe.organizations.Count -ne 1 -or
    $adminMe.organizations[0].role -ne "admin"
) {
    throw "Registration did not create the initial admin membership"
}
$organizationId = $adminMe.organizations[0].organizationId

$invitation = Invoke-RestMethod `
    -WebSession $admin.Session `
    -Method Post `
    -ContentType "application/json" `
    -Headers @{ "x-csrf-token" = $admin.Csrf } `
    -Body (@{
        email = "editor-$suffix@example.com"
        role = "editor"
    } | ConvertTo-Json) `
    "$ApiBase/orgs/$organizationId/invitations"
if (-not $invitation.token -or $invitation.inviteUrl -notmatch "#token=") {
    throw "Invitation did not return its one-time fragment link"
}

$editor = Register-SmokeUser `
    -Email "editor-$suffix@example.com" `
    -OrganizationName "Editor Personal"
$accepted = Invoke-RestMethod `
    -WebSession $editor.Session `
    -Method Post `
    -ContentType "application/json" `
    -Headers @{ "x-csrf-token" = $editor.Csrf } `
    -Body (@{ token = $invitation.token } | ConvertTo-Json) `
    "$ApiBase/invitations/accept"
if (
    $accepted.organizationId -ne $organizationId -or
    $accepted.role -ne "editor"
) {
    throw "Invitation acceptance returned the wrong membership"
}

$deniedStatus = 0
try {
    $null = Invoke-WebRequest `
        -UseBasicParsing `
        -WebSession $editor.Session `
        -Method Post `
        -ContentType "application/json" `
        -Headers @{ "x-csrf-token" = $editor.Csrf } `
        -Body (@{
            email = "denied-$suffix@example.com"
            role = "viewer"
        } | ConvertTo-Json) `
        "$ApiBase/orgs/$organizationId/invitations"
} catch {
    $deniedStatus = [int]$_.Exception.Response.StatusCode
}
if ($deniedStatus -ne 403) {
    throw "Editor member-management request returned $deniedStatus, not 403"
}

$members = Invoke-RestMethod `
    -WebSession $admin.Session `
    "$ApiBase/orgs/$organizationId/members"
if (
    @($members).Count -ne 2 -or
    @($members | Where-Object { $_.role -eq "editor" }).Count -ne 1
) {
    throw "Same-tenant member listing did not expose accepted membership"
}

$outsider = Register-SmokeUser `
    -Email "outsider-$suffix@example.com" `
    -OrganizationName "Outside"
$hiddenStatus = 0
try {
    $null = Invoke-WebRequest `
        -UseBasicParsing `
        -WebSession $outsider.Session `
        "$ApiBase/orgs/$organizationId/members"
} catch {
    $hiddenStatus = [int]$_.Exception.Response.StatusCode
}
if ($hiddenStatus -ne 404) {
    throw "Cross-tenant member request returned $hiddenStatus, not 404"
}

$organizationPath = "/orgs/$organizationId"
$definitions = Invoke-SmokeRequest `
    -User $editor `
    -Method Get `
    -Path "$organizationPath/node-definitions"
$blueprints = Invoke-SmokeRequest `
    -User $editor `
    -Method Get `
    -Path "$organizationPath/blueprints"
if (
    @($definitions.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 7 -or
    @($blueprints.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 2
) {
    throw "System catalog does not list 7 node definitions and 2 blueprints"
}

$workflows = @{}
foreach ($blueprint in $blueprints.Body.items) {
    $created = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$organizationPath/workflows" `
        -Body @{
            name = $blueprint.displayName
            blueprintId = $blueprint.id
        }
    if (
        $created.Status -ne 201 -or
        $created.Body.draft.revision -ne 1 -or
        @($created.Body.draft.graph.nodes).Count -ne @($blueprint.graph.nodes).Count
    ) {
        throw "Applying blueprint $($blueprint.key) returned $($created.Status)"
    }
    $workflows[$blueprint.key] = $created.Body
}

$infrastructure = $workflows["corqty.simulated-infrastructure"]
$infrastructurePath = "$organizationPath/workflows/$($infrastructure.id)"
$saved = Invoke-SmokeRequest `
    -User $editor `
    -Method Put `
    -Path "$infrastructurePath/draft" `
    -Body @{
        revision = 1
        graph = $infrastructure.draft.graph
    }
if ($saved.Status -ne 200 -or $saved.Body.revision -ne 2) {
    throw "Draft save returned $($saved.Status), not revision 2"
}
$published = Invoke-SmokeRequest `
    -User $editor `
    -Method Post `
    -Path "$infrastructurePath/publish" `
    -Body @{ revision = 2 }
if (
    $published.Status -ne 201 -or
    $published.Body.version -ne 1 -or
    $published.Body.checksum -cnotmatch "^[0-9a-f]{64}$"
) {
    throw "Publishing draft revision 2 did not create checksummed version 1"
}
foreach ($refusal in @(
    @{ revision = 2; code = "draft_already_published" },
    @{ revision = 1; code = "draft_revision_conflict" }
)) {
    $refused = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$infrastructurePath/publish" `
        -Body @{ revision = $refusal.revision }
    if ($refused.Status -ne 409 -or $refused.Body.code -ne $refusal.code) {
        throw "Publishing revision $($refusal.revision) did not return 409 $($refusal.code)"
    }
}

$dataProcessing = $workflows["corqty.data-processing"]
$published = Invoke-SmokeRequest `
    -User $editor `
    -Method Post `
    -Path "$organizationPath/workflows/$($dataProcessing.id)/publish" `
    -Body @{ revision = 1 }
if ($published.Status -ne 201 -or $published.Body.version -ne 1) {
    throw "Publishing the data-processing workflow returned $($published.Status)"
}

$outsiderMe = Invoke-RestMethod `
    -WebSession $outsider.Session `
    "$ApiBase/auth/me"
$outsiderOrganizationId = $outsiderMe.organizations[0].organizationId
$foreign = Invoke-SmokeRequest `
    -User $outsider `
    -Method Get `
    -Path "/orgs/$outsiderOrganizationId/workflows/$($infrastructure.id)"
if ($foreign.Status -ne 404) {
    throw "Cross-tenant workflow request returned $($foreign.Status), not 404"
}

$studio = Invoke-WebRequest `
    -UseBasicParsing `
    "$WebBase$infrastructurePath"
if ($studio.StatusCode -ne 200 -or $studio.Content -notmatch "<title>Corqty</title>") {
    throw "Web application did not serve the workflow studio"
}

# Every worker replica runs healthy as uid 10001, unprivileged, on a read-only root
# filesystem and without bind or volume mounts.
$workers = @(
    docker compose --project-name $ProjectName ps --format json worker |
        ConvertFrom-Json
)
$healthyWorkers = @(
    $workers | Where-Object { $_.State -eq "running" -and $_.Health -eq "healthy" }
)
if ($workers.Count -ne $ExpectedWorkers -or $healthyWorkers.Count -ne $ExpectedWorkers) {
    throw "Expected $ExpectedWorkers healthy workers, found $($healthyWorkers.Count) of $($workers.Count)"
}
foreach ($worker in $workers) {
    $workerUser = docker exec $worker.ID id -u
    $isolation = docker inspect `
        --format "{{.HostConfig.Privileged}} {{.HostConfig.ReadonlyRootfs}} {{len .Mounts}}" `
        $worker.ID
    if ($workerUser -ne "10001" -or $isolation -ne "false true 0") {
        throw "Worker $($worker.Name) runs as user $workerUser with '$isolation' (privileged, read-only, mounts)"
    }
}

function New-SmokeTask {
    param(
        [string]$Id,
        [int]$DelayMs = 0,
        $FailureMessage = $null
    )

    @{
        id = $Id
        definition = @{ key = "corqty.mock.task"; version = 1 }
        displayName = $Id
        position = @{ x = 0; y = 0 }
        config = @{
            mode = "fixed"
            output = @{ result = $Id }
            delayMs = $DelayMs
            failureMessage = $FailureMessage
        }
        inputs = @{}
        retry = @{ maxAttempts = 1 }
        timeoutSeconds = 60
    }
}

# Creates a workflow as the editor and publishes the graph as version 1. Returns its path.
function Publish-SmokeWorkflow {
    param(
        [string]$Name,
        [object[]]$Nodes,
        [object[]]$Edges = @()
    )

    $created = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$organizationPath/workflows" `
        -Body @{ name = $Name }
    $path = "$organizationPath/workflows/$($created.Body.id)"
    $saved = Invoke-SmokeRequest `
        -User $editor `
        -Method Put `
        -Path "$path/draft" `
        -Body @{
            revision = 1
            graph = @{
                schemaVersion = 1
                workflowInputSchema = @{ type = "object" }
                nodes = $Nodes
                edges = $Edges
                viewport = @{ x = 0; y = 0; zoom = 1 }
            }
        }
    $published = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$path/publish" `
        -Body @{ revision = 2 }
    if ($published.Status -ne 201) {
        throw "Publishing '$Name' returned $($created.Status), $($saved.Status) and $($published.Status)"
    }
    $path
}

function Start-SmokeRun {
    param(
        [string]$WorkflowPath,
        [hashtable]$RunInput = @{}
    )

    $started = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$WorkflowPath/runs" `
        -Body @{ input = $RunInput }
    if ($started.Status -ne 201 -or $started.Body.status -ne "queued") {
        throw "Starting a run of $WorkflowPath returned $($started.Status)"
    }
    $started.Body
}

# Polls the run for up to 60 s and returns it once it has finished.
function Wait-SmokeRun {
    param([string]$RunId)

    $deadline = (Get-Date).AddSeconds(60)
    while ($true) {
        $run = (Invoke-SmokeRequest -User $editor -Method Get -Path "$organizationPath/runs/$RunId").Body
        if ($run.status -notin @("queued", "running")) {
            return $run
        }
        if ((Get-Date) -gt $deadline) {
            throw "Run $RunId is still $($run.status) after 60 s"
        }
        Start-Sleep -Milliseconds 250
    }
}

function Get-SmokeNode {
    param(
        $Run,
        [string]$NodeId
    )

    $Run.nodes | Where-Object { $_.nodeId -eq $NodeId }
}

# vpc's output reaches rds and deploy. With one slot per worker, rds and deploy overlap
# only when two workers run them.
$infrastructureRun = Wait-SmokeRun (Start-SmokeRun $infrastructurePath @{ region = "us-east-1" }).id
# Applying the blueprint gave the nodes fresh ids, so they are found by definition.
$infrastructureNodes = @{}
foreach ($node in $infrastructureRun.graph.nodes) {
    $infrastructureNodes[$node.definition.key] = Get-SmokeNode $infrastructureRun $node.id
}
$vpc = $infrastructureNodes["corqty.simulated.create-vpc"]
$rds = $infrastructureNodes["corqty.simulated.create-rds"]
$deploy = $infrastructureNodes["corqty.simulated.deploy-service"]
if (
    $infrastructureRun.status -ne "succeeded" -or
    $vpc.input.region -ne "us-east-1" -or
    $rds.input.vpcId -ne $vpc.output.vpcId -or
    $deploy.input.vpcId -ne $vpc.output.vpcId
) {
    throw "The infrastructure run ended $($infrastructureRun.status) without passing the VPC id on"
}
if (
    [DateTimeOffset]$rds.startedAt -ge [DateTimeOffset]$deploy.finishedAt -or
    [DateTimeOffset]$deploy.startedAt -ge [DateTimeOffset]$rds.finishedAt
) {
    throw "rds and deploy did not run at the same time"
}

# A failed node skips only its descendants.
$failurePath = Publish-SmokeWorkflow -Name "Smoke failure" -Nodes @(
    (New-SmokeTask -Id "fail" -FailureMessage "The smoke test fails this node."),
    (New-SmokeTask -Id "child"),
    (New-SmokeTask -Id "unrelated")
) -Edges @(
    @{
        id = "fail-child"
        source = @{ nodeId = "fail"; port = "result" }
        target = @{ nodeId = "child"; port = "input" }
    }
)
$failureRun = Wait-SmokeRun (Start-SmokeRun $failurePath).id
$states = @(
    $failureRun.nodes | Sort-Object nodeId | ForEach-Object { "$($_.nodeId) $($_.state)" }
) -join ", "
if (
    $failureRun.status -ne "failed" -or
    $states -ne "child skipped, fail failed, unrelated succeeded" -or
    (Get-SmokeNode $failureRun "fail").error.code -ne "mock_failure"
) {
    throw "The failure run ended $($failureRun.status) with $states"
}

# Events stream live through the Next.js rewrite: a frame arrives while the node still
# runs, so the rewrite neither buffers nor compresses the stream.
$streamPath = Publish-SmokeWorkflow -Name "Smoke stream" -Nodes @(
    New-SmokeTask -Id "slow" -DelayMs 5000
)
$streamRun = Start-SmokeRun $streamPath
Add-Type -AssemblyName System.Net.Http
$handler = New-Object System.Net.Http.HttpClientHandler
$handler.CookieContainer = $editor.Session.Cookies
$client = New-Object System.Net.Http.HttpClient($handler)
# Browsers accept gzip, and Next.js compresses (and so buffers) any text response that
# lacks Cache-Control: no-transform.
$client.DefaultRequestHeaders.Add("Accept-Encoding", "gzip")
$eventsUrl = "$WebBase/api/v1$organizationPath/runs/$($streamRun.id)/events?after=0"
$clock = [System.Diagnostics.Stopwatch]::StartNew()
$stream = $client.GetAsync(
    $eventsUrl,
    [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead
).GetAwaiter().GetResult()
if (
    [int]$stream.StatusCode -ne 200 -or
    $stream.Content.Headers.ContentType.MediaType -ne "text/event-stream" -or
    $stream.Content.Headers.ContentEncoding.Count -ne 0 -or
    -not $stream.Headers.CacheControl.NoTransform
) {
    throw "The event stream returned $([int]$stream.StatusCode) with Content-Encoding '$($stream.Content.Headers.ContentEncoding)' and Cache-Control '$($stream.Headers.CacheControl)'"
}
$reader = New-Object System.IO.StreamReader(
    $stream.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
)
$frames = @()
$runningAfter = $null
while ($true) {
    $line = $reader.ReadLineAsync()
    if (-not $line.Wait(10000) -or $clock.Elapsed.TotalSeconds -gt 30) {
        throw "The event stream did not end after the run finished"
    }
    if ($null -eq $line.Result) {
        break
    }
    if ($line.Result.StartsWith("data: ")) {
        $frame = $line.Result.Substring(6) | ConvertFrom-Json
        $frames += $frame
        if ($null -eq $runningAfter -and $frame.type -eq "node.state" -and $frame.state -eq "running") {
            $runningAfter = $clock.Elapsed
        }
    }
}
if ($null -eq $runningAfter -or $runningAfter.TotalSeconds -gt 3) {
    throw "The running node.state frame took $runningAfter to arrive through the web service"
}
$sequence = @($frames | ForEach-Object { $_.seq }) -join ","
$last = $frames[-1]
if (
    $sequence -ne ((1..$frames.Count) -join ",") -or
    $last.type -ne "run.state" -or
    $last.state -ne "succeeded"
) {
    throw "The event stream sent seq $sequence and ended with $($last | ConvertTo-Json -Compress)"
}
$streamRun = Wait-SmokeRun $streamRun.id
$caughtUp = New-Object System.Net.Http.HttpRequestMessage(
    [System.Net.Http.HttpMethod]::Get,
    $eventsUrl
)
$caughtUp.Headers.Add("Last-Event-ID", [string]$streamRun.lastEventSeq)
$replayed = $client.SendAsync($caughtUp).GetAwaiter().GetResult()
if ($streamRun.lastEventSeq -ne $last.seq -or [int]$replayed.StatusCode -ne 204) {
    throw "A caught-up stream of the finished run returned $([int]$replayed.StatusCode), not 204"
}
$client.Dispose()

# An admin opts into the trusted shell executor. The shell node reads its resolved inputs
# from CORQTY_INPUT and writes JSON to CORQTY_OUTPUT with jq from the worker image.
$shellEnabled = Invoke-SmokeRequest `
    -User $admin `
    -Method Put `
    -Path "$organizationPath/settings/shell" `
    -Body @{ enabled = $true }
if ($shellEnabled.Status -ne 200 -or $shellEnabled.Body.shellEnabled -ne $true) {
    throw "Enabling the shell executor returned $($shellEnabled.Status)"
}
$greet = New-SmokeTask -Id "greet"
$greet.definition.key = "corqty.shell.command"
$greet.config = @{
    command = 'jq ''{result: {greeting: ("hello " + .input.name)}}'' "$CORQTY_INPUT" > "$CORQTY_OUTPUT"'
    env = @{}
}
$greet.inputs = @{ input = @{ kind = "literal"; value = @{ name = "corqty" } } }
$shellPath = Publish-SmokeWorkflow -Name "Smoke shell" -Nodes @($greet)
$shellRun = Wait-SmokeRun (Start-SmokeRun $shellPath).id
$greeting = (Get-SmokeNode $shellRun "greet").output | ConvertTo-Json -Compress -Depth 8
if ($shellRun.status -ne "succeeded" -or $greeting -ne '{"result":{"greeting":"hello corqty"}}') {
    throw "The shell run ended $($shellRun.status) with output $greeting"
}

$foreignRun = Invoke-SmokeRequest `
    -User $outsider `
    -Method Get `
    -Path "/orgs/$outsiderOrganizationId/runs/$($infrastructureRun.id)"
if ($foreignRun.Status -ne 404) {
    throw "Cross-tenant run request returned $($foreignRun.Status), not 404"
}

$audit = Invoke-RestMethod `
    -WebSession $admin.Session `
    "$ApiBase/orgs/$organizationId/audit-events?limit=50"
$actions = @($audit.items | ForEach-Object { $_.action })
foreach ($action in @(
    "organization.created",
    "invitation.created",
    "invitation.accepted",
    "workflow.created",
    "workflow.draft_saved",
    "workflow.published",
    "organization.shell_enabled",
    "run.started"
)) {
    if ($actions -notcontains $action) {
        throw "Audit log is missing $action"
    }
}

$services = docker compose --project-name $ProjectName ps --format json |
    ConvertFrom-Json
$required = @("api", "postgres", "web")
foreach ($service in $required) {
    $entry = $services | Where-Object { $_.Service -eq $service }
    if ($null -eq $entry -or $entry.State -ne "running") {
        throw "Missing running service: $service"
    }
    if ($entry.Health -and $entry.Health -ne "healthy") {
        throw "Unhealthy service: $service"
    }
}

foreach ($service in @("migrate", "api")) {
    $container = docker compose --project-name $ProjectName ps --all --quiet $service
    $user = docker inspect --format "{{.Config.User}}" $container
    if ($user -ne "10001:10001") {
        throw "The $service container runs as user '$user', not 10001:10001"
    }
}
