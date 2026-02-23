$dll = 'C:\Users\Owner\Documents\GitHub\Huidu\hdplayer_extracted\HCatNet.dll'
$bytes = [System.IO.File]::ReadAllBytes($dll)
$enc8 = [System.Text.Encoding]::UTF8

# Extract ALL ASCII strings >= 5 chars
$run = New-Object System.Collections.Generic.List[byte]
$results = New-Object System.Collections.Generic.List[string]

for ($i = 0; $i -lt $bytes.Length; $i++) {
    $b = $bytes[$i]
    if ($b -ge 0x20 -and $b -le 0x7e) {
        $run.Add($b)
    } else {
        if ($run.Count -ge 5) {
            $results.Add($enc8.GetString($run.ToArray()))
        }
        $run.Clear()
    }
}

Write-Host "Total strings: $($results.Count)"

# Show ALL strings that look like method names or XML
$xml_strings = $results | Where-Object {
    $_ -match '<|>|method=|guid=|xml|GetDevice|GetAll|GetHard|SetBright|OpenScreen|CloseScreen|Reboot|Upgrade|GetFile|Delete|Switch|SetTime|GetTime|Brightness|program'
}
Write-Host "`nXML/method strings:"
$xml_strings | ForEach-Object { Write-Host "  $_" }

# Show strings that start with 'k' (command constants)
Write-Host "`nAll k-constants:"
$results | Where-Object { $_ -match '^k[A-Z]' } | ForEach-Object { Write-Host "  $_" }

# Show strings that look like error messages
Write-Host "`nError/status strings:"
$results | Where-Object { $_ -match 'error|Error|fail|Fail|success|invalid|Invalid|parse|Parse' } | ForEach-Object { Write-Host "  $_" }

# Show strings around 0x20nn command codes (look for hex-like values)
Write-Host "`nVersion/number strings:"
$results | Where-Object { $_ -match '0x2[0-9]|kVersion|version[0-9]|\bversion\b' } | ForEach-Object { Write-Host "  $_" }
