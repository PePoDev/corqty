param(
    [string]$ProjectName = "corqty",
    [string]$ApiBase = "http://localhost:8080/api/v1",
    [string]$WebBase = "http://localhost:3000"
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
    @($definitions.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 6 -or
    @($blueprints.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 2
) {
    throw "System catalog does not list 6 node definitions and 2 blueprints"
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
    "workflow.published"
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
