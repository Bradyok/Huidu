# Proper UDP discovery: bind to 9526 (where device sends announces),
# echo announce back to device:9527, then TCP connect + BoxStreamInit

$udp = New-Object System.Net.Sockets.UdpClient
$udp.Client.SetSocketOption([System.Net.Sockets.SocketOptionLevel]::Socket,
    [System.Net.Sockets.SocketOptionName]::ReuseAddress, $true)
$udp.Client.Bind([System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 9526))
$udp.EnableBroadcast = $true
$udp.Client.ReceiveTimeout = 5000

Write-Host "Listening for UDP announces on port 9526..."

# Also send a trigger to device:9527 to prompt announce
$udpSend = New-Object System.Net.Sockets.UdpClient
$udpSend.EnableBroadcast = $true
$trigger = [byte[]](0x02, 0x00, 0x01, 0x00)
$dest = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Broadcast, 9527)
$udpSend.Send($trigger, $trigger.Length, $dest) | Out-Null
Write-Host "Sent UDP trigger to broadcast:9527"
$udpSend.Close()

$remoteEP = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
$deviceIP = $null
try {
    $data = $udp.Receive([ref]$remoteEP)
    $hex = ($data | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
    Write-Host "Got UDP from $($remoteEP.Address):$($remoteEP.Port) ($($data.Length) bytes): $hex"
    $deviceIP = $remoteEP.Address

    if ($data.Length -eq 21) {
        Write-Host "It's a 21-byte announce, echoing to $($remoteEP.Address):9527"
        $udpReply = New-Object System.Net.Sockets.UdpClient
        $deviceAddr = [System.Net.IPEndPoint]::new($remoteEP.Address, 9527)
        $udpReply.Send($data, $data.Length, $deviceAddr) | Out-Null
        Write-Host "Echoed announce"

        # Wait for full info
        $udp.Client.ReceiveTimeout = 3000
        try {
            $info = $udp.Receive([ref]$remoteEP)
            Write-Host "Got full info ($($info.Length) bytes)"
        } catch {
            Write-Host "No full info response"
        }
        $udpReply.Close()
    }
} catch {
    Write-Host "No UDP announce received (using device IP from known address)"
    $deviceIP = [System.Net.IPAddress]::Parse('192.168.1.104')
}
$udp.Close()

# Now TCP connect
Start-Sleep -Milliseconds 200
Write-Host "`nConnecting TCP to $deviceIP:9527..."
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
try {
    $tcp.Connect($deviceIP, 9527)
    Write-Host "TCP Connected"
    $stream = $tcp.GetStream()
    $buf = New-Object byte[] 256

    # Read initial packet with generous timeout
    $stream.ReadTimeout = 5000
    try {
        $n = $stream.Read($buf, 0, 256)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            Write-Host "Initial: $hex"
            if ($buf[2] -eq 0x60) {
                Write-Host "-> TcpHeartbeatAnswer, sending TcpHeartbeatAsk"
                $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
                $stream.Flush()
                Start-Sleep -Milliseconds 100
            }
        }
    } catch {
        Write-Host "No initial packet"
    }

    # Send BoxStreamInit
    Write-Host "Sending BoxStreamInit..."
    $stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
    $stream.Flush()

    # Read response
    $stream.ReadTimeout = 5000
    try {
        $n = $stream.Read($buf, 0, 256)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            $cmd = [uint16](($buf[3] -shl 8) -bor $buf[2])
            Write-Host "Response: $hex (cmd=0x$($cmd.ToString('X4')))"
        } else {
            Write-Host "Device closed connection"
        }
    } catch {
        Write-Host "Read timeout"
    }
} catch {
    Write-Host "TCP failed: $_"
} finally {
    $tcp.Close()
}
