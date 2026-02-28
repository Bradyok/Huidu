Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead("BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin")
$entry = $zip.Entries | Where-Object { $_.Name -like "BoxPlayer*.bin" }
Write-Host "Entry name: $($entry.Name)"
Write-Host "Entry size: $($entry.Length)"
$stream = $entry.Open()
$buf = New-Object byte[] 700
$read = $stream.Read($buf, 0, 700)
Write-Host "Bytes read: $read"
$hex676 = ($buf[676..682] | ForEach-Object { $_.ToString("x2") }) -join " "
Write-Host "Bytes at 676-682: $hex676"
$hex0 = ($buf[0..7] | ForEach-Object { $_.ToString("x2") }) -join " "
Write-Host "Bytes at 0-7: $hex0"
# Check for gzip magic 1f 8b
for ($i = 670; $i -le 690; $i++) {
    if ($buf[$i] -eq 0x1f -and $buf[$i+1] -eq 0x8b) {
        Write-Host "GZIP MAGIC FOUND at offset $i"
    }
}
$stream.Close()
$zip.Dispose()
