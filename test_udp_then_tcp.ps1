# UDP discovery first, then immediate TCP BoxStreamInit
Write-Host "=== Phase 1: UDP Discovery ==="

# Bind UDP to receive device's announce on port 9527
$udp = New-Object System.Net.Sockets.UdpClient
$udp.ExclusiveAddressUse = $false
$udp.Client.SetSocketOption([System.Net.Sockets.SocketOptionLevel]::Socket,
    [System.Net.Sockets.SocketOptionName]::ReuseAddress, $true)
$udp.Client.Bind([System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 9527))
$udp.EnableBroadcast = $true

# Send trigger: SearchDeviceAsk [02 00 01 00]
$trigger = [byte[]](0x02, 0x00, 0x01, 0x00)
$udp.Send($trigger, $trigger.Length, [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Broadcast, 9527)) | Out-Null
Write-Host "Sent UDP trigger"

# Wait for 21-byte device announce
$udp.Client.ReceiveTimeout = 5000
$remoteEP = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
$announce = $null
$deviceIP = [System.Net.IPAddress]::Parse('192.168.1.104')

try {
    for ($i = 0; $i -lt 5; $i++) {
        $data = $udp.Receive([ref]$remoteEP)
        $hex = ($data | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        Write-Host "UDP from $($remoteEP.Address):$($remoteEP.Port): $hex ($($data.Length) bytes)"
        
        # Skip our own broadcast
        if ($remoteEP.Address.ToString() -eq '192.168.1.100') {
            Write-Host "  (own broadcast, skip)"
            continue
        }
        
        if ($data.Length -eq 21) {
            $announce = $data
            $deviceIP = $remoteEP.Address
            Write-Host "Got 21-byte announce from $deviceIP"
            
            # Echo it back to the device on port 9527
            $devEP = [System.Net.IPEndPoint]::new($deviceIP, 9527)
            $udp.Send($announce, $announce.Length, $devEP) | Out-Null
            Write-Host "Echoed announce back"
            break
        }
        break
    }
} catch {
    Write-Host "UDP error: $_"
}
$udp.Close()

Write-Host "`n=== Phase 2: TCP BoxStreamInit (IMMEDIATELY) ==="
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect($deviceIP, 9527)
Write-Host "TCP connected at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 65536

# Send BoxStreamInit IMMEDIATELY (before device's heartbeat timer fires)
$stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
$stream.Flush()
Write-Host "BoxStreamInit sent at $(Get-Date -Format 'HH:mm:ss.fff')"

# Wait for response (up to 10s)
$task = $stream.ReadAsync($buf, 0, 65536)
$completed = $task.Wait(10000)
if ($completed) {
    $n = $task.Result
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Response: $n bytes: $hex [cmd=0x$($cmd.ToString('X4'))]"
        if ($cmd -eq 0x0201) {
            Write-Host "*** SUCCESS! BoxStreamInitAck received! ***"
        }
    } else {
        Write-Host "FIN received (n=0)"
    }
} else {
    Write-Host "Timeout - no response in 10s"
}
$tcp.Close()
