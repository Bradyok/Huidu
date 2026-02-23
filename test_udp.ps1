# Test TCP alive check
$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 10001)
    Write-Host "TCP 10001: CONNECTED - device is alive"
    $tcp.Close()
} catch {
    Write-Host ("TCP 10001: FAILED - " + $_.Exception.Message)
}

# Listen on port 9526
$udpListen = New-Object System.Net.Sockets.UdpClient(9526)
$udpListen.Client.ReceiveTimeout = 4000

# Send triggers to device
$udpSend = New-Object System.Net.Sockets.UdpClient
$udpSend.EnableBroadcast = $true
$trigOld = [byte[]](2, 0, 1, 0)
$trigNew = [byte[]](0, 0, 0, 1, 1, 0)

Write-Host "Sending discovery triggers to 192.168.1.104:9527 and broadcast:9527..."
$udpSend.Send($trigOld, $trigOld.Length, '192.168.1.104', 9527) | Out-Null
$udpSend.Send($trigNew, $trigNew.Length, '192.168.1.104', 9527) | Out-Null
$udpSend.Send($trigOld, $trigOld.Length, '255.255.255.255', 9527) | Out-Null
$udpSend.Close()
Write-Host "Waiting for response on port 9526..."

$ep = New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Any, 0)
try {
    $bytes = $udpListen.Receive([ref]$ep)
    Write-Host ("Received " + $bytes.Length + " bytes from " + $ep.ToString())
    Write-Host ([BitConverter]::ToString($bytes))

    # If 21-byte announce, echo back to device:9527
    if ($bytes.Length -eq 21) {
        Write-Host "Short announce - echoing back to device port 9527..."
        $udpEcho = New-Object System.Net.Sockets.UdpClient
        $udpEcho.Send($bytes, $bytes.Length, $ep.Address.ToString(), 9527) | Out-Null
        $udpEcho.Close()

        # Wait for full response
        $udpListen.Client.ReceiveTimeout = 3000
        try {
            $bytes2 = $udpListen.Receive([ref]$ep)
            Write-Host ("Full response: " + $bytes2.Length + " bytes from " + $ep.ToString())
            Write-Host ([BitConverter]::ToString($bytes2))
        } catch {
            Write-Host "No full response received after echo"
        }
    }
} catch {
    Write-Host "No response on port 9526 after 4s"
}
$udpListen.Close()
