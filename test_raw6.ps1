# Test with longer timeout to see if device EVENTUALLY sends TcpHeartbeatAnswer
$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "Connected"
    $stream = $tcp.GetStream()
    $stream.ReadTimeout = 8000   # 8 second timeout

    $buf = New-Object byte[] 128
    try {
        $n = $stream.Read($buf, 0, 128)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            Write-Host "Device sent $n bytes: $hex"

            # If it's TcpHeartbeatAnswer (04 00 60 00), respond with TcpHeartbeatAsk
            if ($n -eq 4 -and $buf[2] -eq 0x60 -and $buf[3] -eq 0x00) {
                Write-Host "Got TcpHeartbeatAnswer! Responding with TcpHeartbeatAsk..."
                $hb = [byte[]](0x04, 0x00, 0x5F, 0x00)
                $stream.Write($hb, 0, $hb.Length)

                # Now send BoxStreamInit
                Start-Sleep -Milliseconds 50
                $init = [byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00)
                $stream.Write($init, 0, $init.Length)
                Write-Host "Sent BoxStreamInit"

                $stream.ReadTimeout = 3000
                $n2 = $stream.Read($buf, 0, 128)
                if ($n2 -gt 0) {
                    $hex2 = ($buf[0..($n2-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                    Write-Host "Device replied to BoxStreamInit: $n2 bytes: $hex2"
                } else {
                    Write-Host "Device closed after BoxStreamInit"
                }
            }
        } else {
            Write-Host "Device closed connection"
        }
    } catch {
        Write-Host "Read timeout (device sent nothing in 8s)"
    }
} catch {
    Write-Host "Connect failed: $_"
} finally {
    $tcp.Close()
}
