# Simpler approach: use async reads with Task to detect RST vs FIN vs data
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "Connected at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 1024

# Wait up to 12 seconds for initial heartbeat
$stream.ReadTimeout = 12000
try {
    $n = $stream.Read($buf, 0, 1024)
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Initial: $hex [cmd=0x$($cmd.ToString('X4'))] at $(Get-Date -Format 'HH:mm:ss.fff')"
        if ($cmd -eq 0x0060) {
            Write-Host "TcpHeartbeatAnswer! Responding..."
            $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
            $stream.Flush()
            Start-Sleep -Milliseconds 200
        }
    } elseif ($n -eq 0) {
        Write-Host "n=0 on initial read"
    }
} catch {
    Write-Host "Initial timeout"
}

Write-Host "Sending BoxStreamInit at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
$stream.Flush()

# Quick reads to catch all responses
for ($i = 0; $i -lt 10; $i++) {
    $stream.ReadTimeout = 1000
    try {
        $n = $stream.Read($buf, 0, 1024)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
            Write-Host "Read[$i]: $hex [cmd=0x$($cmd.ToString('X4'))]"
            if ($cmd -eq 0x0201) {
                Write-Host "BoxStreamInitAck!"
                break
            }
        } else {
            Write-Host "Read[$i]: n=0 (closed)"
            break
        }
    } catch [System.IO.IOException] {
        Write-Host "Read[$i]: IOException (RST?)"
        break
    } catch {
        Write-Host "Read[$i]: timeout (500ms)"
        break
    }
}

$tcp.Close()
