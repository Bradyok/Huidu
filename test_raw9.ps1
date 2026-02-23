# Full test: BoxStreamInit -> TcpHeartbeatAnswer -> TcpHeartbeatAsk -> BoxStreamInit again
$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "TCP Connected"
    $stream = $tcp.GetStream()
    $buf = New-Object byte[] 256

    function ReadPacket($timeout_ms) {
        $stream.ReadTimeout = $timeout_ms
        try {
            $n = $stream.Read($buf, 0, 256)
            if ($n -gt 0) {
                $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                Write-Host "  RECV $n bytes: $hex"
                return $buf[0..($n-1)]
            } else {
                Write-Host "  RECV: connection closed (n=0)"
                return $null
            }
        } catch {
            Write-Host "  RECV: timeout"
            return @()
        }
    }

    function SendPkt($bytes) {
        $hex = ($bytes | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        Write-Host "  SEND: $hex"
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush()
    }

    # Try reading first (maybe device sends something)
    Write-Host "1. Waiting for initial packet from device (1s)..."
    $pkt = ReadPacket(1000)

    if ($pkt -ne $null -and $pkt.Count -ge 4 -and $pkt[2] -eq 0x60) {
        Write-Host "   -> Got TcpHeartbeatAnswer, responding..."
        SendPkt([byte[]](0x04, 0x00, 0x5F, 0x00))
        Start-Sleep -Milliseconds 100
    } elseif ($pkt -ne $null -and $pkt.Count -eq 0) {
        Write-Host "   -> Timeout, proceeding"
    }

    # Send BoxStreamInit
    Write-Host "2. Sending BoxStreamInit [0,0,0,0]..."
    SendPkt([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00))

    # Read all responses for 5 seconds
    Write-Host "3. Reading responses..."
    for ($i = 0; $i -lt 10; $i++) {
        $pkt = ReadPacket(2000)
        if ($pkt -eq $null) { break }
        if ($pkt.Count -eq 0) {
            Write-Host "   -> Timeout"
            break
        }

        # Check command
        if ($pkt.Count -ge 4) {
            $cmd = [uint16](($pkt[3] -shl 8) -bor $pkt[2])
            Write-Host "   -> Cmd: 0x$($cmd.ToString('X4'))"

            switch ($cmd) {
                0x0060 {
                    Write-Host "   -> TcpHeartbeatAnswer received, responding with TcpHeartbeatAsk"
                    SendPkt([byte[]](0x04, 0x00, 0x5F, 0x00))
                    Start-Sleep -Milliseconds 200
                    Write-Host "   -> Sending BoxStreamInit again..."
                    SendPkt([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00))
                }
                0x0201 {
                    Write-Host "   -> BoxStreamInitAck! Success!"
                    break
                }
                default {
                    Write-Host "   -> Unknown response"
                }
            }
        }
    }
} catch {
    Write-Host "Error: $_"
} finally {
    $tcp.Close()
}
