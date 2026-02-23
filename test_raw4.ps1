# Test SdkServiceAsk (0x2001) with version 0x07000000
# Packet: [08 00 01 20 00 00 00 07] = length=8, cmd=0x2001, payload=[00,00,00,07]
$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect('192.168.1.104', 9527)
    Write-Host "Connected to port 9527"
    $stream = $tcp.GetStream()
    $stream.ReadTimeout = 3000

    $pkt = [byte[]](0x08, 0x00, 0x01, 0x20, 0x00, 0x00, 0x00, 0x07)
    $stream.Write($pkt, 0, $pkt.Length)
    Write-Host "Sent SdkServiceAsk [08 00 01 20 00 00 00 07]"

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
        Write-Host "Read timeout/error (still connected)"
    }
} catch {
    Write-Host "Connect failed: $_"
} finally {
    $tcp.Close()
}
