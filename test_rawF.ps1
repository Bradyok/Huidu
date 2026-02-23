# Wait up to 15s for device's TcpHeartbeatAnswer, then full BoxStreamInit exchange
Write-Host "Connecting..."
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "Connected at $(Get-Date -Format 'HH:mm:ss')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 512

# Wait up to 15 seconds for the device's initial TcpHeartbeatAnswer
Write-Host "Waiting for TcpHeartbeatAnswer (up to 15s)..."
$stream.ReadTimeout = 15000
try {
    $n = $stream.Read($buf, 0, 512)
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Got: $hex [cmd=0x$($cmd.ToString('X4'))] at $(Get-Date -Format 'HH:mm:ss')"

        if ($cmd -eq 0x0060) {
            Write-Host "TcpHeartbeatAnswer! Responding with TcpHeartbeatAsk..."
            $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
            $stream.Flush()
            Start-Sleep -Milliseconds 200

            Write-Host "Sending BoxStreamInit..."
            $stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
            $stream.Flush()

            $stream.ReadTimeout = 5000
            $n2 = $stream.Read($buf, 0, 512)
            if ($n2 -gt 0) {
                $hex2 = ($buf[0..($n2-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                $cmd2 = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
                Write-Host "BoxStreamInit response: $hex2 [cmd=0x$($cmd2.ToString('X4'))]"
            } else {
                Write-Host "Connection closed after BoxStreamInit"
            }
        } else {
            Write-Host "Unexpected initial packet"
        }
    } else {
        Write-Host "Connection closed immediately"
    }
} catch {
    Write-Host "Timeout after 15s - no initial packet from device"
}

$tcp.Close()
