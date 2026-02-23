# Test: respond to MULTIPLE heartbeat cycles before sending BoxStreamInit
# Hypothesis: device requires N heartbeat cycles before accepting BoxStreamInit
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "Connected at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 1024

$heartbeatCount = 0
$sentInit = $false

for ($i = 0; $i -lt 20; $i++) {
    $stream.ReadTimeout = 12000
    $n = 0
    $gotData = $false
    try {
        $n = $stream.Read($buf, 0, 1024)
        $gotData = $true
    } catch [System.IO.IOException] {
        Write-Host "[$i] IOException (RST)"
        break
    } catch {
        Write-Host "[$i] Timeout (12s) - no more data"
        break
    }

    if (-not $gotData) { break }

    if ($n -eq 0) {
        Write-Host "[$i] n=0 (connection closed by device)"
        break
    }

    $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
    $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
    Write-Host "[$i] Recv: $hex [cmd=0x$($cmd.ToString('X4'))] at $(Get-Date -Format 'HH:mm:ss.fff')"

    if ($cmd -eq 0x0060) {
        $heartbeatCount++
        Write-Host "[$i] TcpHeartbeatAnswer #$heartbeatCount - responding..."
        $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
        $stream.Flush()
        Write-Host "[$i] Responded."

        if ($heartbeatCount -ge 2) {
            Write-Host "[$i] Have $heartbeatCount heartbeats - sending BoxStreamInit..."
            $stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
            $stream.Flush()
            $sentInit = $true
            Write-Host "[$i] Sent BoxStreamInit at $(Get-Date -Format 'HH:mm:ss.fff')"
        }
    } elseif ($cmd -eq 0x0201) {
        Write-Host "[$i] *** BoxStreamInitAck received! SUCCESS! ***"
        break
    } elseif ($sentInit) {
        Write-Host "[$i] Got cmd 0x$($cmd.ToString('X4')) after BoxStreamInit"
    }
}

$tcp.Close()
Write-Host "Done. Total heartbeats received: $heartbeatCount"
