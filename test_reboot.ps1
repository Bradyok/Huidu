# Try various reboot method names on port 10001 to find one that works
$methods = @("RebootDevice", "Reboot", "DeviceReboot", "SystemReboot", "kReboot", "reboot")

foreach ($method in $methods) {
    $tcp = New-Object System.Net.Sockets.TcpClient
    $tcp.NoDelay = $true
    try {
        $tcp.Connect('192.168.1.104', 10001)
    } catch {
        Write-Host "Cannot connect to port 10001: $_"
        break
    }
    $stream = $tcp.GetStream()
    $buf = New-Object byte[] 65536

    # SdkServiceAsk: [u16 LE 8][u16 LE 0x2001][u32 LE 0x07000000]
    $stream.Write([byte[]](0x08, 0x00, 0x01, 0x20, 0x00, 0x00, 0x00, 0x07), 0, 8)
    $stream.Flush()

    # Read SdkServiceAnswer
    $stream.ReadTimeout = 3000
    try {
        $n = $stream.Read($buf, 0, 65536)
        if ($n -ge 4) {
            $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
            $payload = $buf[4..($n-1)]
            # SdkServiceAnswer gives back the server GUID in the payload (32+ chars)
            $serverGuid = [System.Text.Encoding]::UTF8.GetString($payload)
            Write-Host "Handshake OK cmd=0x$($cmd.ToString('X4')) guid(first 16)=$($serverGuid.Substring(0, [Math]::Min(16, $serverGuid.Length)))"
        }
    } catch {
        Write-Host "Handshake timeout"
        $tcp.Close()
        continue
    }

    # Build SDK request XML
    $clientGuid = [guid]::NewGuid().ToString().ToUpper()
    $xml = "<?xml version=""1.0"" encoding=""utf-8""?><sdk guid=""$clientGuid""><in method=""$method""></in></sdk>"
    $xmlBytes = [System.Text.Encoding]::UTF8.GetBytes($xml)
    
    # SdkCmdAsk: [u16 LE total_len][u16 LE 0x2003][u32 LE xml_len][u32 LE 0][xml]
    $totalLen = 4 + 4 + 4 + $xmlBytes.Length  # 4 hdr + 4 total + 4 idx + xml
    $pkt = New-Object byte[] ($totalLen)
    $pkt[0] = [byte]($totalLen -band 0xFF)
    $pkt[1] = [byte](($totalLen -shr 8) -band 0xFF)
    $pkt[2] = 0x03; $pkt[3] = 0x20  # cmd 0x2003
    $pkt[4] = [byte]($xmlBytes.Length -band 0xFF)
    $pkt[5] = [byte](($xmlBytes.Length -shr 8) -band 0xFF)
    $pkt[6] = [byte](($xmlBytes.Length -shr 16) -band 0xFF)
    $pkt[7] = [byte](($xmlBytes.Length -shr 24) -band 0xFF)
    $pkt[8] = 0; $pkt[9] = 0; $pkt[10] = 0; $pkt[11] = 0  # chunk index 0
    [System.Array]::Copy($xmlBytes, 0, $pkt, 12, $xmlBytes.Length)

    $stream.Write($pkt, 0, $pkt.Length)
    $stream.Flush()

    # Read response
    $stream.ReadTimeout = 3000
    try {
        $n = $stream.Read($buf, 0, 65536)
        if ($n -ge 4) {
            $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
            if ($cmd -eq 0x2000) {
                # SdkErrorAnswer
                $errCode = [uint16]([uint16]($buf[4]) -bor ([uint16]($buf[5]) -shl 8))
                Write-Host "Method '$method': ERROR code=$errCode"
            } elseif ($cmd -eq 0x2004) {
                # SdkCmdAnswer
                $resp = [System.Text.Encoding]::UTF8.GetString($buf, 12, [Math]::Max(0, $n - 12))
                Write-Host "Method '$method': SUCCESS response=$resp"
            } else {
                $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
                Write-Host "Method '$method': UNEXPECTED cmd=0x$($cmd.ToString('X4')) hex=$hex"
            }
        } else {
            Write-Host "Method '$method': Short response ($n bytes)"
        }
    } catch {
        Write-Host "Method '$method': Timeout/error - $_"
    }

    $tcp.Close()
    Start-Sleep -Milliseconds 100
}
