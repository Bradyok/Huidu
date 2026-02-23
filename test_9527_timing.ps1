# Test full BoxStream sequence with correct timing (heartbeat at ~5s)
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "TCP connected at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 65536

# Wait up to 10s for heartbeat (device sends it at ~5s)
$task = $stream.ReadAsync($buf, 0, 65536)
$completed = $task.Wait(10000)
if ($completed) {
    $n = $task.Result
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Got heartbeat: $hex [cmd=0x$($cmd.ToString('X4'))] at $(Get-Date -Format 'HH:mm:ss.fff')"
        
        if ($cmd -eq 0x0060) {
            Write-Host "Responding with TcpHeartbeatAsk..."
            $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
            $stream.Flush()
            
            # Wait a moment then send BoxStreamInit
            Start-Sleep -Milliseconds 100
            Write-Host "Sending BoxStreamInit..."
            $stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
            $stream.Flush()
            
            # Wait for BoxStreamInitAck
            $task2 = $stream.ReadAsync($buf, 0, 65536)
            $completed2 = $task2.Wait(10000)
            if ($completed2) {
                $n2 = $task2.Result
                if ($n2 -gt 0) {
                    $hex2 = ($buf[0..($n2-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                    $cmd2 = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
                    Write-Host "BoxStreamInit response: $hex2 [cmd=0x$($cmd2.ToString('X4'))] at $(Get-Date -Format 'HH:mm:ss.fff')"
                    if ($cmd2 -eq 0x0201) {
                        Write-Host "SUCCESS! BoxStreamInitAck received!"
                    }
                } else {
                    Write-Host "FIN after BoxStreamInit"
                }
            } else {
                Write-Host "Timeout waiting for BoxStreamInitAck"
            }
        }
    }
} else {
    Write-Host "No heartbeat in 10s"
}
$tcp.Close()
