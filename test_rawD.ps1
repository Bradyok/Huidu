# Hypothesis: sending UDP trigger first enables TCP BoxStreamInit
# Minimal test: send UDP, then TCP + BoxStreamInit

# Send UDP discovery trigger to device
$udpSend = New-Object System.Net.Sockets.UdpClient
$udpSend.EnableBroadcast = $true
$trigger = [byte[]](0x02, 0x00, 0x01, 0x00)
$dest = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Broadcast, 9527)
$udpSend.Send($trigger, $trigger.Length, $dest) | Out-Null
Write-Host "Sent UDP trigger"
$udpSend.Close()
Start-Sleep -Milliseconds 100

# TCP connect + BoxStreamInit
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "TCP Connected"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 512

# No initial wait, just send BoxStreamInit directly
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
    } else {
        Write-Host "Connection closed (n=0)"
    }
} catch {
    Write-Host "Timeout"
}
$tcp.Close()
