# Send UDP, wait 5 seconds, then TCP + BoxStreamInit
$udpSend = New-Object System.Net.Sockets.UdpClient
$udpSend.EnableBroadcast = $true
$trigger = [byte[]](0x02, 0x00, 0x01, 0x00)
$udpSend.Send($trigger, $trigger.Length, [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Broadcast, 9527)) | Out-Null
Write-Host "Sent UDP trigger"
$udpSend.Close()

Write-Host "Waiting 5 seconds..."
Start-Sleep -Seconds 5

$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "TCP Connected"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 512

# Wait briefly for initial heartbeat
$stream.ReadTimeout = 2000
try {
    $n = $stream.Read($buf, 0, 512)
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Initial: $hex [cmd=0x$($cmd.ToString('X4'))]"
        if ($cmd -eq 0x0060) {
            $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
            $stream.Flush()
            Write-Host "Responded to heartbeat"
            Start-Sleep -Milliseconds 200
        }
    }
} catch {
    Write-Host "No initial packet"
}

$stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
$stream.Flush()
Write-Host "Sent BoxStreamInit"

$stream.ReadTimeout = 5000
try {
    $n = $stream.Read($buf, 0, 512)
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Response: $hex [cmd=0x$($cmd.ToString('X4'))]"

        if ($cmd -eq 0x0060) {
            Write-Host "Got TcpHeartbeatAnswer, responding + resending BoxStreamInit"
            $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
            Start-Sleep -Milliseconds 100
            $stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
            $stream.Flush()
            $stream.ReadTimeout = 5000
            $n2 = $stream.Read($buf, 0, 512)
            if ($n2 -gt 0) {
                $hex2 = ($buf[0..($n2-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                $cmd2 = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
                Write-Host "Response2: $hex2 [cmd=0x$($cmd2.ToString('X4'))]"
            }
        }
    } else {
        Write-Host "Connection closed"
    }
} catch {
    Write-Host "Timeout"
}
$tcp.Close()
