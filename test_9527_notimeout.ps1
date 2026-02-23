# Test port 9527 with NO read timeout - just wait and see what happens
$tcp = New-Object System.Net.Sockets.TcpClient
$tcp.NoDelay = $true
$tcp.Connect('192.168.1.104', 9527)
Write-Host "TCP connected at $(Get-Date -Format 'HH:mm:ss.fff')"
$stream = $tcp.GetStream()
$buf = New-Object byte[] 65536

# No read timeout - wait indefinitely for data
# Use async read to implement our own timeout
$task = $stream.ReadAsync($buf, 0, 65536)
$completed = $task.Wait(15000)  # Wait up to 15 seconds
if ($completed) {
    $n = $task.Result
    if ($n -gt 0) {
        $hex = ($buf[0..($n-1)] | ForEach-Object { '{0:X2}' -f $_ }) -join '-'
        $cmd = [uint16]([uint16]($buf[2]) -bor ([uint16]($buf[3]) -shl 8))
        Write-Host "Got $n bytes: $hex [cmd=0x$($cmd.ToString('X4'))] at $(Get-Date -Format 'HH:mm:ss.fff')"
    } else {
        Write-Host "Got n=0 (FIN) at $(Get-Date -Format 'HH:mm:ss.fff')"
    }
} else {
    Write-Host "No data received in 15 seconds (still connected)"
}
$tcp.Close()
