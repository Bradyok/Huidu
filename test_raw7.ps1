# Full sequence with 1.5 second delay between TcpHeartbeatAsk and BoxStreamInit
# (mirroring the 1.4s delay seen in the pcap)
$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "Connected"
    $stream = $tcp.GetStream()
    $stream.ReadTimeout = 10000

    # Wait for device's TcpHeartbeatAnswer
    $buf = New-Object byte[] 256
    try {
        $n = $stream.Read($buf, 0, 256)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            Write-Host "Device sent $n bytes: $hex"
        }
    } catch {
        Write-Host "No initial data from device (timeout)"
        $tcp.Close()
        return
    }

    # Step 1: Respond with TcpHeartbeatAsk
    $hbask = [byte[]](0x04, 0x00, 0x5F, 0x00)
    $stream.Write($hbask, 0, $hbask.Length)
    $stream.Flush()
    Write-Host "Sent TcpHeartbeatAsk"

    # Wait 1.5 seconds (matching pcap timing)
    Start-Sleep -Milliseconds 1500

    # Step 2: Send BoxStreamInit
    $init = [byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00)
    $stream.Write($init, 0, $init.Length)
    $stream.Flush()
    Write-Host "Sent BoxStreamInit"

    # Read response
    $stream.ReadTimeout = 5000
    try {
        $n = $stream.Read($buf, 0, 256)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            Write-Host "Response: $n bytes: $hex"
        } else {
            Write-Host "Device closed connection after BoxStreamInit"
        }
    } catch {
        Write-Host "Read timeout after BoxStreamInit"
    }
} catch {
    Write-Host "Error: $_"
} finally {
    $tcp.Close()
}
