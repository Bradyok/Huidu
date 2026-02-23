# Full Telnet interaction with IAC handling
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 23)
Write-Host "Telnet connected at $(Get-Date -Format 'HH:mm:ss')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 4096

# Respond to IAC negotiation:
# FF FD 01 = IAC DO ECHO => respond FF FC 01 (IAC WONT ECHO) 
# FF FD 1F = IAC DO NAWS => respond FF FB 1F (IAC WILL NAWS) + window size
# FF FB 01 = IAC WILL ECHO => respond FF FD 01 (IAC DO ECHO)
# FF FB 03 = IAC WILL SUPPRESS-GO-AHEAD => respond FF FD 03 (IAC DO SGA)

$stream.ReadTimeout = 3000
try {
    $n = $stream.Read($buf, 0, 4096)
    Write-Host "Got $n bytes of IAC negotiation"
}
catch { Write-Host "Timeout on IAC" }

# Send IAC responses:
# WONT ECHO, WILL NAWS (with terminal size 80x24), DO ECHO, DO SUPPRESS-GO-AHEAD
$resp = [byte[]](
    0xFF, 0xFC, 0x01,   # IAC WONT ECHO
    0xFF, 0xFB, 0x1F,   # IAC WILL NAWS
    0xFF, 0xFA, 0x1F, 0x00, 0x50, 0x00, 0x18, 0xFF, 0xF0,  # NAWS subneg 80x24
    0xFF, 0xFD, 0x01,   # IAC DO ECHO
    0xFF, 0xFD, 0x03    # IAC DO SUPPRESS-GO-AHEAD
)
$stream.Write($resp, 0, $resp.Length)
$stream.Flush()

# Read login prompt
$stream.ReadTimeout = 5000
try {
    $n = $stream.Read($buf, 0, 4096)
    if ($n -gt 0) {
        $text = [System.Text.Encoding]::ASCII.GetString($buf, 0, $n) -replace '[^\x20-\x7E\r\n]', '.'
        Write-Host "Prompt: '$text'"
    }
}
catch { Write-Host "No login prompt" }

# Try login: send "root" + Enter
$login = [System.Text.Encoding]::ASCII.GetBytes("root`r`n")
$stream.Write($login, 0, $login.Length)
$stream.Flush()
Write-Host "Sent: root"

# Read response (password prompt or shell)
$stream.ReadTimeout = 3000
$allText = ""
for ($i = 0; $i -lt 5; $i++) {
    try {
        $n = $stream.Read($buf, 0, 4096)
        if ($n -gt 0) {
            $text = [System.Text.Encoding]::ASCII.GetString($buf, 0, $n) -replace '[^\x20-\x7E\r\n]', '.'
            $allText += $text
            Write-Host "Response[$i]: '$text'"
            if ($text -match "Password|password|#|\$|>") { break }
        }
    } catch { break }
}

# If we got a password prompt, try empty password
if ($allText -match "Password|password") {
    Write-Host "Got password prompt - trying empty password"
    $stream.Write([byte[]](0x0D, 0x0A), 0, 2)
    $stream.Flush()
    Start-Sleep -Milliseconds 1000
    try {
        $n = $stream.Read($buf, 0, 4096)
        $text = [System.Text.Encoding]::ASCII.GetString($buf, 0, $n) -replace '[^\x20-\x7E\r\n]', '.'
        Write-Host "After empty password: '$text'"
        if ($text -match "#|\$") {
            Write-Host "GOT SHELL! Sending reboot..."
            $rebootCmd = [System.Text.Encoding]::ASCII.GetBytes("reboot`r`n")
            $stream.Write($rebootCmd, 0, $rebootCmd.Length)
            $stream.Flush()
        }
    } catch {}
}

$tcp.Close()
