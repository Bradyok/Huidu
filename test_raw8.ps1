# Test: UDP discovery echo FIRST, then TCP BoxStreamInit
# Theory: device only accepts BoxStreamInit from IPs that completed UDP discovery

# Step 1: UDP discovery - send trigger and echo announce
$udp = New-Object System.Net.Sockets.UdpClient(9527)  # bind to port 9527
$udp.Client.SetSocketOption([System.Net.Sockets.SocketOptionLevel]::Socket,
    [System.Net.Sockets.SocketOptionName]::ReuseAddress, $true)
$udp.EnableBroadcast = $true

Write-Host "Sending UDP discovery trigger to broadcast..."
# Old format trigger: [02 00 01 00]
$trigger = [byte[]](0x02, 0x00, 0x01, 0x00)
$broadcast = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Broadcast, 9527)
$udp.Send($trigger, $trigger.Length, $broadcast) | Out-Null

# Wait for device announce and echo it back
$udp.Client.ReceiveTimeout = 3000
$remoteEP = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
try {
    $announce = $udp.Receive([ref]$remoteEP)
    $hex = ($announce | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
    Write-Host "Got UDP announce from $($remoteEP.Address):$($remoteEP.Port) ($($announce.Length) bytes)"
    Write-Host "Announce: $hex"

    # Echo back to device port 9527
    $deviceAddr = [System.Net.IPEndPoint]::new($remoteEP.Address, 9527)
    $udp.Send($announce, $announce.Length, $deviceAddr) | Out-Null
    Write-Host "Echoed announce back to device"

    # Wait for full info response
    Start-Sleep -Milliseconds 500
    try {
        $info = $udp.Receive([ref]$remoteEP)
        $infoHex = ($info[0..15] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        Write-Host "Got full info packet ($($info.Length) bytes): $infoHex..."
    } catch {
        Write-Host "No full info response"
    }
} catch {
    Write-Host "No UDP announce received: $_"
}
$udp.Close()

# Small delay
Start-Sleep -Milliseconds 200

# Step 2: TCP connect and send BoxStreamInit
Write-Host "`nNow connecting TCP..."
$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "TCP Connected"
    $stream = $tcp.GetStream()
    $stream.ReadTimeout = 5000

    # Wait for TcpHeartbeatAnswer or just proceed
    $buf = New-Object byte[] 256
    try {
        $n = $stream.Read($buf, 0, 256)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            Write-Host "Device sent: $hex"
            if ($buf[2] -eq 0x60) {
                # TcpHeartbeatAnswer - respond
                $hb = [byte[]](0x04, 0x00, 0x5F, 0x00)
                $stream.Write($hb, 0, $hb.Length)
                Write-Host "Responded with TcpHeartbeatAsk"
                Start-Sleep -Milliseconds 100
            }
        }
    } catch {
        Write-Host "No initial packet from device (OK)"
    }

    # Send BoxStreamInit
    $init = [byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00)
    $stream.Write($init, 0, $init.Length)
    Write-Host "Sent BoxStreamInit"

    $stream.ReadTimeout = 5000
    try {
        $n = $stream.Read($buf, 0, 256)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            Write-Host "Device replied: $hex (BoxStreamInitAck expected: 06-00-01-02-00-00)"
        } else {
            Write-Host "Device closed connection"
        }
    } catch {
        Write-Host "Read timeout"
    }
} catch {
    Write-Host "TCP connect failed: $_"
} finally {
    $tcp.Close()
}
