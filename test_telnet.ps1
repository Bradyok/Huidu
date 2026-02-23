# Test telnet port - connect and see what banner/prompt we get
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 23)
Write-Host "Telnet connected"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 4096
$stream.ReadTimeout = 3000
try {
    $n = $stream.Read($buf, 0, 4096)
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join ' '
        $text = [System.Text.Encoding]::ASCII.GetString($buf, 0, $n) -replace '[^\x20-\x7E\r\n]', '?'
        Write-Host "Received $n bytes:"
        Write-Host "HEX: $hex"
        Write-Host "TXT: $text"
    } else {
        Write-Host "Connection closed immediately (n=0)"
    }
} catch {
    Write-Host "Timeout/error: $_"
}
$tcp.Close()
