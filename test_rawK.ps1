# Test sequence: heartbeat exchange → SdkServiceAsk → BoxStreamInit
# Hypothesis: fresh connection requires version negotiation before BoxStreamInit
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "Connected at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 4096

function Read-Pkt {
    param([int]$timeoutMs = 8000)
    $stream.ReadTimeout = $timeoutMs
    $n = 0
    try { $n = $stream.Read($buf, 0, 4096) } catch { return $null }
    if ($n -eq 0) { return "CLOSED" }
    $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join ' '
    $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
    return [PSCustomObject]@{ Hex=$hex; Cmd=$cmd; Bytes=$buf[0..($n-1)]; N=$n }
}

# Step 1: Wait for TcpHeartbeatAnswer (0x0060)
Write-Host "Waiting for TcpHeartbeatAnswer..."
$pkt = Read-Pkt 12000
if ($null -eq $pkt) { Write-Host "Timeout"; exit }
if ($pkt -eq "CLOSED") { Write-Host "CLOSED on initial read"; exit }
Write-Host "Got: $($pkt.Hex) [cmd=0x$($pkt.Cmd.ToString('X4'))]"

if ($pkt.Cmd -eq 0x0060) {
    Write-Host "TcpHeartbeatAnswer received. Responding with TcpHeartbeatAsk (0x005F)..."
    $stream.Write([byte[]](0x04, 0x00, 0x5F, 0x00), 0, 4)
    $stream.Flush()
    Write-Host "Responded."

    # Step 2: Send SdkServiceAsk (0x2001) with version 0x07000000
    Write-Host "Sending SdkServiceAsk (0x2001) with version 0x07000000..."
    # Packet: [len=8 LE][cmd=0x2001 LE][payload=0x07000000 LE]
    # len = 4+4 = 8
    $pktBytes = [byte[]](0x08, 0x00, 0x01, 0x20, 0x00, 0x00, 0x00, 0x07)
    $stream.Write($pktBytes, 0, 8)
    $stream.Flush()
    Write-Host "Sent SdkServiceAsk."

    # Step 3: Wait for SdkServiceAnswer (0x2002) or whatever the device sends
    Write-Host "Waiting for response..."
    $pkt2 = Read-Pkt 5000
    if ($null -eq $pkt2) { Write-Host "Timeout waiting for SdkServiceAnswer" }
    elseif ($pkt2 -eq "CLOSED") { Write-Host "Device CLOSED after SdkServiceAsk" }
    else {
        Write-Host "Got: $($pkt2.Hex) [cmd=0x$($pkt2.Cmd.ToString('X4'))]"
        if ($pkt2.Cmd -eq 0x2002) {
            Write-Host "SdkServiceAnswer received!"

            # Step 4: Now try BoxStreamInit
            Write-Host "Sending BoxStreamInit..."
            $stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
            $stream.Flush()

            $pkt3 = Read-Pkt 5000
            if ($null -eq $pkt3) { Write-Host "Timeout waiting for BoxStreamInitAck" }
            elseif ($pkt3 -eq "CLOSED") { Write-Host "Device CLOSED after BoxStreamInit" }
            else {
                Write-Host "Got: $($pkt3.Hex) [cmd=0x$($pkt3.Cmd.ToString('X4'))]"
                if ($pkt3.Cmd -eq 0x0201) { Write-Host "*** BoxStreamInitAck! SUCCESS! ***" }
            }
        } elseif ($pkt2.Cmd -eq 0x2000) {
            Write-Host "SdkErrorAnswer received (error)"
        } else {
            Write-Host "Unexpected response to SdkServiceAsk"
            # Still try BoxStreamInit
            Write-Host "Trying BoxStreamInit anyway..."
            $stream.Write([byte[]](0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00), 0, 8)
            $stream.Flush()
            $pkt3 = Read-Pkt 3000
            if ($null -eq $pkt3) { Write-Host "Timeout" }
            elseif ($pkt3 -eq "CLOSED") { Write-Host "CLOSED" }
            else { Write-Host "Got: $($pkt3.Hex) [cmd=0x$($pkt3.Cmd.ToString('X4'))]" }
        }
    }
} else {
    Write-Host "Unexpected first packet, cmd=0x$($pkt.Cmd.ToString('X4'))"
}

$tcp.Close()
Write-Host "Done."
