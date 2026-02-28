# Rename the new .bin so Compress-Archive uses the right filename
Copy-Item "fw_repack_work\BoxPlayer_7_11_18_0.bin_new" "fw_repack_work\BoxPlayer_7_11_18_0.bin" -Force
Write-Host "Copied. File size: $((Get-Item 'fw_repack_work\BoxPlayer_7_11_18_0.bin').Length)"

# Use Compress-Archive (PowerShell built-in)
$output = "BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0_patched.zbin"
Remove-Item -Force $output -ErrorAction SilentlyContinue
Compress-Archive -Path "fw_repack_work\BoxPlayer_7_11_18_0.bin" -DestinationPath $output -CompressionLevel NoCompression
Write-Host "Created: $output ($((Get-Item $output).Length) bytes)"

# Verify using zip listing
& tar -tf $output 2>&1
