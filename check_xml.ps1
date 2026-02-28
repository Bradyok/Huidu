Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead("BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin")
$entry = $zip.Entries | Where-Object { $_.Name -like "BoxPlayer*.bin" }
Write-Host "Entry name: $($entry.Name)"
$stream = $entry.Open()
# Read full header (678 bytes)
$buf = New-Object byte[] 678
$read = $stream.Read($buf, 0, 678)
Write-Host "Header bytes read: $read"
# Bytes 0-17 are magic/version info, bytes 18-677 are XML
$xmlBytes = $buf[18..677]
$xmlStr = [System.Text.Encoding]::UTF8.GetString($xmlBytes)
Write-Host "XML Header:"
Write-Host $xmlStr
$stream.Close()
$zip.Dispose()
