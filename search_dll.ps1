$dll = 'C:\Users\Owner\Documents\GitHub\Huidu\hdsign-extracted\HDCommunicate.dll'
$bytes = [System.IO.File]::ReadAllBytes($dll)
$enc = [System.Text.Encoding]::UTF8
$found = @()

# Search for key XML/protocol strings in ASCII
for ($i = 0; $i -lt $bytes.Length - 30; $i++) {
    # Look for "method=" (ASCII)
    if ($bytes[$i] -eq 0x6d -and $bytes[$i+1] -eq 0x65 -and $bytes[$i+2] -eq 0x74 -and $bytes[$i+3] -eq 0x68 -and $bytes[$i+4] -eq 0x6f -and $bytes[$i+5] -eq 0x64) {
        $end = [Math]::Min($i + 60, $bytes.Length)
        $s = $enc.GetString($bytes[$i..($end-1)])
        $found += "ASCII@${i}: $s"
    }
    # Look for "GetDev" (ASCII)
    if ($bytes[$i] -eq 0x47 -and $bytes[$i+1] -eq 0x65 -and $bytes[$i+2] -eq 0x74 -and $bytes[$i+3] -eq 0x44 -and $bytes[$i+4] -eq 0x65 -and $bytes[$i+5] -eq 0x76) {
        $end = [Math]::Min($i + 40, $bytes.Length)
        $s = $enc.GetString($bytes[$i..($end-1)])
        $found += "GetDev@${i}: $s"
    }
    # Look for "sdk" (ASCII)
    if ($bytes[$i] -eq 0x73 -and $bytes[$i+1] -eq 0x64 -and $bytes[$i+2] -eq 0x6b) {
        $end = [Math]::Min($i + 40, $bytes.Length)
        $s = $enc.GetString($bytes[$i..($end-1)])
        $found += "sdk@${i}: $s"
    }
}

Write-Host "Found $($found.Count) matches:"
$found | Select-Object -First 50
