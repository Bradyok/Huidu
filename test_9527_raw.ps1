# Raw low-level test of port 9527
for ($attempt = 0; $attempt -lt 3; $attempt++) {
    Write-Host "`n=== Attempt $attempt ==="
    $tcp = New-Object System.Net.Sockets.TcpClient
    $tcp.NoDelay = $true
    $tcp.ReceiveBufferSize = 65536
    
    try {
        $tcp.Connect('192.168.1.104', 9527)
        Write-Host "TCP handshake complete at $(Get-Date -Format 'HH:mm:ss.fff')"
    } catch {
        Write-Host "Failed to connect: $_"
        continue
    }
    
    $stream = $tcp.GetStream()
    $buf = New-Object byte[] 65536
    
    # Try reading with multiple very short timeouts to catch anything before FIN
    $received = @()
    for ($r = 0; $r -lt 5; $r++) {
        $stream.ReadTimeout = 500
        try {
            $n = $stream.Read($buf, 0, 65536)
            if ($n -gt 0) {
                $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                Write-Host "  Read[$r]: $n bytes: $hex at $(Get-Date -Format 'HH:mm:ss.fff')"
                $received += $n
            } elseif ($n -eq 0) {
                Write-Host "  Read[$r]: n=0 (FIN received) at $(Get-Date -Format 'HH:mm:ss.fff')"
                break
            }
        } catch [System.IO.IOException] {
            Write-Host "  Read[$r]: IOException (RST?) at $(Get-Date -Format 'HH:mm:ss.fff')"
            break
        } catch {
            Write-Host "  Read[$r]: Timeout at $(Get-Date -Format 'HH:mm:ss.fff')"
            break
        }
    }
    
    $tcp.Close()
    Start-Sleep -Milliseconds 500
}
