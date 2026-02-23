# Quick TCP port scan of common ports
$ip = "192.168.1.104"
$ports = @(22, 23, 80, 443, 6104, 8080, 8888, 9527, 10001, 10002, 5555, 554, 21, 4001, 9999, 1234, 7000, 7001, 4444, 8000)
foreach ($port in $ports) {
    $tcp = New-Object System.Net.Sockets.TcpClient
    try {
        $tcp.Connect($ip, $port)
        if ($tcp.Connected) {
            Write-Host "PORT $port OPEN"
        }
    } catch {}
    $tcp.Close()
}
Write-Host "Scan complete"
