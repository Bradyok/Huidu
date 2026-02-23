# Try sending TcpHeartbeatAsk (0x005F) first — packet = [04 00 5F 00] (length=4, cmd=0x005F)
$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "Connected"
    $stream = $tcp.GetStream()
    $stream.ReadTimeout = 3000

    # Send TcpHeartbeatAsk: [u16LE length=4][u16LE cmd=0x005F]
    $heartbeat = [byte[]](0x04, 0x00, 0x5F, 0x00)
    $stream.Write($heartbeat, 0, $heartbeat.Length)
    Write-Host "Sent TcpHeartbeatAsk"

    $buf = New-Object byte[] 128
    try {
        $n = $stream.Read($buf, 0, 128)
        if ($n -gt 0) {
            $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
            Write-Host "Device replied $n bytes: $hex"
        } else {
            Write-Host "Device closed connection (n=0)"
        }
    } catch {
        Write-Host "Read timeout/error: $_"
    }
} catch {
    Write-Host "Connect failed: $_"
} finally {
    $tcp.Close()
}
