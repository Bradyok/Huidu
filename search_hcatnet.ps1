$dll = 'C:\Users\Owner\Documents\GitHub\Huidu\hdplayer_extracted\HCatNet.dll'
$bytes = [System.IO.File]::ReadAllBytes($dll)
$enc8 = [System.Text.Encoding]::UTF8
$enc16 = [System.Text.Encoding]::Unicode

Write-Host "DLL size: $($bytes.Length) bytes"

# Extract ASCII strings >= 8 chars
$run = New-Object System.Collections.Generic.List[byte]
$results = @()

for ($i = 0; $i -lt $bytes.Length; $i++) {
    $b = $bytes[$i]
    if ($b -ge 0x20 -and $b -le 0x7e) {
        $run.Add($b)
    } else {
        if ($run.Count -ge 8) {
            $s = $enc8.GetString($run.ToArray())
            $results += $s
        }
        $run.Clear()
    }
}

Write-Host "Total ASCII strings: $($results.Count)"

# Filter for relevant strings
$proto = $results | Where-Object {
    $_ -match 'method|GetDevice|Bright|Program|upgrade|xml|sdk guid|in method|version|2001|2002|2003|2004|8001|8003' -or
    $_ -match 'GetAll|GetHard|GetTime|SetBright|Screen|SetWifi|GetFile|DeleteFile|Reboot'
}
Write-Host "Protocol strings ($($proto.Count)):"
$proto | Select-Object -First 80 | ForEach-Object { Write-Host "  $_" }

Write-Host ""
Write-Host "=== First 200 strings ==="
$results | Select-Object -First 200 | ForEach-Object { Write-Host "  $_" }
