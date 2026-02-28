Add-Type -AssemblyName System.IO.Compression.FileSystem

$workDir = "fw_repack_work"
$newBin = "$workDir\BoxPlayer_7_11_18_0.bin_new"
$outputZbin = "BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0_patched.zbin"
$entryName = "BoxPlayer_7_11_18_0.bin"

Write-Host "Creating $outputZbin with entry $entryName..."
Write-Host "Source file size: $((Get-Item $newBin).Length) bytes"

Remove-Item -Force $outputZbin -ErrorAction SilentlyContinue

$zip = [System.IO.Compression.ZipFile]::Open($outputZbin, [System.IO.Compression.ZipArchiveMode]::Create)
$entry = $zip.CreateEntry($entryName, [System.IO.Compression.CompressionLevel]::NoCompression)
$entryStream = $entry.Open()
$fileStream = [System.IO.File]::OpenRead($newBin)
$fileStream.CopyTo($entryStream)
$fileStream.Close()
$entryStream.Close()
$zip.Dispose()

Write-Host "Done! $outputZbin size: $((Get-Item $outputZbin).Length) bytes"
# Verify: open and check entry
$verify = [System.IO.Compression.ZipFile]::OpenRead($outputZbin)
$entries = $verify.Entries
Write-Host "Entries in zip:"
foreach ($e in $entries) {
    Write-Host "  $($e.Name) ($($e.Length) bytes)"
}
$verify.Dispose()
