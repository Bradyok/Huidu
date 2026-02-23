# Try sending BoxStreamInit (0x0200) directly: [08 00 00 02 00 00 00 00]
# packet = length=8(total), cmd=0x0200, payload=[0,0,0,0]
$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "Connected"
    $stream = $tcp.GetStream()
    $stream.ReadTimeout = 3000

    # Send BoxStreamInit: [u16LE length=8][u16LE cmd=0x0200][payload: 00 00 00 00]
    $pkt = [byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00)
    $stream.Write($pkt, 0, $pkt.Length)
    Write-Host "Sent BoxStreamInit [08 00 00 02 00 00 00 00]"

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
