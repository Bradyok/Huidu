$ip = "192.168.1.104"
# Scan additional port ranges
$ports = @(7000..7010) + @(8000..8010) + @(8080, 8088, 8090, 8443, 9000, 9001, 9100, 4001, 5001, 6001, 7001, 3000, 4000, 1337, 2000, 2001)
foreach ($port in $ports) {
    $tcp = New-Object System.Net.Sockets.TcpClient
    $tcp.ReceiveTimeout = 500
    try {
        $async = $tcp.BeginConnect($ip, $port, $null, $null)
        $success = $async.AsyncWaitHandle.WaitOne(500)
        if ($success -and $tcp.Connected) {
            Write-Host "PORT $port OPEN"
        }
    } catch {}
    $tcp.Close()
}
Write-Host "Scan complete"
