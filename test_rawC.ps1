# Full exchange: BoxStreamInit -> TcpHeartbeatAnswer -> TcpHeartbeatAsk -> BoxStreamInit again

$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "Connected"
    $stream = $tcp.GetStream()
    $buf = New-Object byte[] 512

    function Recv($timeout) {
        $stream.ReadTimeout = $timeout
        try {
            $n = $stream.Read($buf, 0, 512)
            if ($n -gt 0) {
                $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                $cmd = [uint16](($buf[3] -shl 8) -bor $buf[2])
                Write-Host "  RECV $n bytes: $hex [cmd=0x$($cmd.ToString('X4'))]"
                return ,$buf[0..($n-1)]
            } else {
                Write-Host "  RECV: closed (n=0)"
                return $null
            }
        } catch {
            Write-Host "  RECV: timeout"
            return ,@()
        }
    }

    function Send($bytes) {
        $hex = ($bytes | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16](($bytes[3] -shl 8) -bor $bytes[2])
        Write-Host "  SEND $($bytes.Length) bytes: $hex [cmd=0x$($cmd.ToString('X4'))]"
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush()
    }

    # Step 1: BoxStreamInit
    Write-Host "-- Step 1: BoxStreamInit --"
    Send([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00))

    # Read responses, handle TcpHeartbeatAnswer
    for ($round = 0; $round -lt 5; $round++) {
        Write-Host "-- Round $round --"
        $pkt = Recv(3000)

        if ($pkt -eq $null) {
            Write-Host "Connection closed"
            break
        }
        if ($pkt.Count -eq 0) {
            Write-Host "Timeout"
            break
        }

        $cmd = [uint16](($pkt[3] -shl 8) -bor $pkt[2])

        if ($cmd -eq 0x0060) {
            Write-Host "  -> TcpHeartbeatAnswer, responding with TcpHeartbeatAsk + BoxStreamInit"
            Send([byte[]](0x04, 0x00, 0x5F, 0x00))
            Start-Sleep -Milliseconds 200
            Send([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00))
        } elseif ($cmd -eq 0x0201) {
            Write-Host "  -> BoxStreamInitAck! Now send BatchRequest XML..."
            # Send BoxStreamData with a simple batch request
            $xml = '<?xml version="1.0" encoding="utf-8"?><sdk guid="##GUID"><in method="GetDeviceName"/></sdk>'
            $xmlBytes = [System.Text.Encoding]::UTF8.GetBytes($xml)
            $payload = [byte[]](0x00, 0x00) + $xmlBytes
            $totalLen = [uint16]($payload.Length + 4)
            $packet = [byte[]]($totalLen -band 0xFF, ($totalLen -shr 8) -band 0xFF, 0x02, 0x02) + $payload
            Send($packet)
            # Read response
            $resp = Recv(10000)
            if ($resp -ne $null -and $resp.Count -gt 0) {
                Write-Host "  -> Got BoxStreamData response!"
                # Extract XML (skip 2-byte direction prefix)
                $xmlResp = [System.Text.Encoding]::UTF8.GetString($resp[6..($resp.Count-1)])
                Write-Host "  XML: $xmlResp"
            }
            break
        } else {
            Write-Host "  -> Unknown cmd=$cmd"
        }
    }
} catch {
    Write-Host "Error: $_"
} finally {
    $tcp.Close()
}
