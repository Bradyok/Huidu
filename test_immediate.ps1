# Test: send BoxStreamInit IMMEDIATELY on connect, no heartbeat wait
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "Connected at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 65536

# Immediately send BoxStreamInit
Write-Host "Sending BoxStreamInit immediately..."
$stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
$stream.Flush()
Write-Host "BoxStreamInit sent at $(Get-Date -Format 'HH:mm:ss.fff')"

# Wait for response
$task = $stream.ReadAsync($buf, 0, 65536)
$completed = $task.Wait(5000)
if ($completed) {
    $n = $task.Result
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Response: $n bytes: $hex [cmd=0x$($cmd.ToString('X4'))] at $(Get-Date -Format 'HH:mm:ss.fff')"
        if ($cmd -eq 0x0201) {
            Write-Host "SUCCESS! BoxStreamInitAck!"
        } elseif ($cmd -eq 0x0060) {
            Write-Host "Got TcpHeartbeatAnswer - responding and sending BoxStreamInit again"
            $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
            $stream.Flush()
            Start-Sleep -Milliseconds 50
            $stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
            $stream.Flush()
            # Wait for BoxStreamInitAck
            $task2 = $stream.ReadAsync($buf, 0, 65536)
            $c2 = $task2.Wait(5000)
            if ($c2 -and $task2.Result -gt 0) {
                $n2 = $task2.Result
                $hex2 = ($buf[0..($n2-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                $cmd2 = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
                Write-Host "Response2: $hex2 [cmd=0x$($cmd2.ToString('X4'))]"
            }
        }
    } elseif ($n -eq 0) {
        Write-Host "FIN at $(Get-Date -Format 'HH:mm:ss.fff')"
    }
} else {
    Write-Host "Timeout 5s"
}
$tcp.Close()
