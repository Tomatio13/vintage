# VINTAGE_INTEGRATION_ID=codex
# VINTAGE_INTEGRATION_VERSION=4
# Installed by VINTAGE. Reports the native session id and state to VINTAGE's
# hook IPC. Does nothing when the VINTAGE hook environment is absent.
param([string]$Action = "")

switch ($Action) {
    "session" { }
    "idle" { }
    "working" { }
    "blocked" { }
    "released" { }
    default { exit 0 }
}
if ($env:VINTAGE_HOOK_ENV -ne "1") { exit 0 }
if (-not $env:VINTAGE_HOOK_PORT) { exit 0 }
if (-not $env:VINTAGE_HOOK_TOKEN) { exit 0 }
if (-not $env:VINTAGE_PANE_ID) { exit 0 }
$agent = if ($env:VINTAGE_AGENT) { $env:VINTAGE_AGENT } else { "codex" }

try {
    $input = [Console]::In.ReadToEnd()
} catch {
    exit 0
}
if (-not $input.Trim()) { exit 0 }

try {
    $hook = $input | ConvertFrom-Json
} catch {
    exit 0
}

# State actions report the mapped state; only session carries an id.
# released clears the pane's agent, so the agent name is omitted.
$state = $null
$sessionId = $null
$reportAgent = $agent
if ($Action -eq "released") {
    $state = "released"
    $reportAgent = $null
} elseif ($Action -eq "session") {
    if ($hook.hook_event_name -ne "SessionStart") { exit 0 }
    if (-not $hook.session_id) { exit 0 }
    $sessionId = [string]$hook.session_id
} else {
    $state = $Action
}

$report = @{
    paneId     = $env:VINTAGE_PANE_ID
    generation = [int]$env:VINTAGE_GENERATION
    source     = "$agent-hook"
    authToken  = $env:VINTAGE_HOOK_TOKEN
}
if ($reportAgent) { $report.agent = $reportAgent }
if ($state) { $report.state = $state }
if ($sessionId) { $report.sessionId = $sessionId }
$report = $report | ConvertTo-Json -Compress

try {
    $client = [System.Net.Sockets.TcpClient]::new()
    $client.Connect("127.0.0.1", [int]$env:VINTAGE_HOOK_PORT)
    $client.ReceiveTimeout = 500
    $stream = $client.GetStream()
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($report + "`n"))
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Flush()
    $client.Close()
} catch {
    exit 0
}
