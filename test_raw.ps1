$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "Connected"
    $stream = $tcp.GetStream()
    $stream.ReadTimeout = 2000
    $buf = New-Object byte[] 64
    try {
        $n = $stream.Read($buf, 0, 64)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            Write-Host "Device sent $n bytes: $hex"
        } else {
            Write-Host "Device closed connection immediately (n=0)"
        }
    } catch {
        Write-Host "Read timeout or error: $_"
    }
} catch {
    Write-Host "Connect failed: $_"
} finally {
    $tcp.Close()
}
