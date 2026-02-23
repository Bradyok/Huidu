# PROPER discovery: trigger -> receive 21-byte announce -> echo -> receive full info -> TCP + BoxStreamInit

Write-Host "=== Phase 1: UDP Discovery ==="

# Bind UDP to port 9527 (same port device uses)
$udp = New-Object System.Net.Sockets.UdpClient
$udp.ExclusiveAddressUse = $false
$udp.Client.SetSocketOption([System.Net.Sockets.SocketOptionLevel]::Socket,
    [System.Net.Sockets.SocketOptionName]::ReuseAddress, $true)
$udp.Client.Bind([System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 9527))
$udp.EnableBroadcast = $true

# Send search trigger: [02 00 01 00] (old format) to broadcast:9527
$trigger = [byte[]](0x02, 0x00, 0x01, 0x00)
$broadcast = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Broadcast, 9527)
$udp.Send($trigger, $trigger.Length, $broadcast) | Out-Null
Write-Host "Sent UDP trigger to 255.255.255.255:9527"

# Wait for device's 21-byte announce
$udp.Client.ReceiveTimeout = 5000
$remoteEP = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
$deviceIP = $null

try {
    # Drain packets until we find one that's NOT from ourselves
    for ($i = 0; $i -lt 10; $i++) {
        $data = $udp.Receive([ref]$remoteEP)
        $hex = ($data | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        Write-Host "Got UDP from $($remoteEP.Address):$($remoteEP.Port) ($($data.Length) bytes): $hex"

        # Skip our own broadcast (192.168.1.100 = our IP)
        if ($remoteEP.Address.ToString() -eq '192.168.1.100') {
            Write-Host "  (that's our own broadcast, skipping)"
            continue
        }

        $deviceIP = $remoteEP.Address
        Write-Host "Device IP: $deviceIP"

        if ($data.Length -eq 21) {
            Write-Host "It's a 21-byte announce! Echoing back to $($deviceIP):9527..."
            $devAddr = [System.Net.IPEndPoint]::new($deviceIP, 9527)
            $udp.Send($data, $data.Length, $devAddr) | Out-Null
            Write-Host "Echoed announce"

            # Wait for full info response
            $udp.Client.ReceiveTimeout = 3000
            try {
                $info = $udp.Receive([ref]$remoteEP)
                Write-Host "Got full info ($($info.Length) bytes) from $($remoteEP.Address)"
            } catch {
                Write-Host "No full info response"
            }
        }
        break
    }
} catch {
    Write-Host "UDP timeout: $_"
}
$udp.Close()

if (-not $deviceIP) {
    $deviceIP = [System.Net.IPAddress]::Parse('192.168.1.104')
    Write-Host "Using known device IP: $deviceIP"
}

# Short pause
Start-Sleep -Milliseconds 300

Write-Host "`n=== Phase 2: TCP BoxStreamInit ==="
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect($deviceIP, 9527)
Write-Host "TCP Connected at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 1024

# Wait for initial TcpHeartbeatAnswer
$stream.ReadTimeout = 12000
try {
    $n = $stream.Read($buf, 0, 1024)
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Initial: $hex [cmd=0x$($cmd.ToString('X4'))] at $(Get-Date -Format 'HH:mm:ss.fff')"
        if ($cmd -eq 0x0060) {
            $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
            $stream.Flush()
            Write-Host "Responded with TcpHeartbeatAsk, waiting 200ms..."
            Start-Sleep -Milliseconds 200
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
    $n = $stream.Read($buf, 0, 1024)
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "BoxStreamInit response: $hex [cmd=0x$($cmd.ToString('X4'))]"
        if ($cmd -eq 0x0201) {
            Write-Host "SUCCESS! BoxStreamInitAck received!"
        }
    } else {
        Write-Host "Device closed (n=0)"
    }
} catch {
    Write-Host "Timeout"
}
$tcp.Close()
