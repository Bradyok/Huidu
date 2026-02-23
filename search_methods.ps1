$dlls = @(
    'C:\Users\Owner\Documents\GitHub\Huidu\hdplayer_extracted\hcommon.dll',
    'C:\Users\Owner\Documents\GitHub\Huidu\hdplayer_extracted\MainWindow.dll',
    'C:\Users\Owner\Documents\GitHub\Huidu\hdplayer_extracted\NetIOServices.dll',
    'C:\Users\Owner\Documents\GitHub\Huidu\hdsign-extracted\HDCommunicate.dll'
)

foreach ($dll in $dlls) {
    if (-not (Test-Path $dll)) { continue }
    Write-Host ""
    Write-Host "=== $([System.IO.Path]::GetFileName($dll)) ==="
    $bytes = [System.IO.File]::ReadAllBytes($dll)
    $enc8 = [System.Text.Encoding]::UTF8
    $run = New-Object System.Collections.Generic.List[byte]
    $found = New-Object System.Collections.Generic.List[string]

    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $b = $bytes[$i]
        if ($b -ge 0x20 -and $b -le 0x7e) {
            $run.Add($b)
        } else {
            if ($run.Count -ge 5) {
                $s = $enc8.GetString($run.ToArray())
                # Look for SDK method names (typically PascalCase or GetXxx/SetXxx)
                if ($s -match '^(Get|Set|Add|Update|Delete|Open|Close|Switch|Reboot|Sync|Login|Logout|Unlock|Check|Reload|Smart|Screen|Clear|Upload|Download|Execute|Excute|Firmware|Upgrade|Scan|Get[A-Z])[A-Za-z]+$') {
                    $found.Add($s)
                }
            }
            $run.Clear()
        }
    }
    $found | Sort-Object -Unique | ForEach-Object { Write-Host "  $_" }
}
