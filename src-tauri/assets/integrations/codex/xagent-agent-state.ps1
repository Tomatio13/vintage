# XAGENT_INTEGRATION_ID=codex
# XAGENT_INTEGRATION_VERSION=1
# Installed by xagent. Reports the native session id to xagent's hook IPC.
# Does nothing when the xagent hook environment is absent.
param([string]$Action = "")

if ($Action -ne "session") { exit 0 }
if ($env:XAGENT_HOOK_ENV -ne "1") { exit 0 }
if (-not $env:XAGENT_HOOK_PORT) { exit 0 }
if (-not $env:XAGENT_HOOK_TOKEN) { exit 0 }
if (-not $env:XAGENT_PANE_ID) { exit 0 }

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
if ($hook.hook_event_name -ne "SessionStart") { exit 0 }
if (-not $hook.session_id) { exit 0 }

$report = @{
    paneId     = $env:XAGENT_PANE_ID
    generation = [int]$env:XAGENT_GENERATION
    source     = "codex-hook"
    agent      = "codex"
    sessionId  = [string]$hook.session_id
    authToken  = $env:XAGENT_HOOK_TOKEN
} | ConvertTo-Json -Compress

try {
    $client = [System.Net.Sockets.TcpClient]::new()
    $client.Connect("127.0.0.1", [int]$env:XAGENT_HOOK_PORT)
    $client.ReceiveTimeout = 500
    $stream = $client.GetStream()
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($report + "`n"))
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Flush()
    $client.Close()
} catch {
    exit 0
}
