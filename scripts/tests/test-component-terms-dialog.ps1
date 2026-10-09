# Windows-only synthetic control contract. This runs a test-local copy of the
# actual embedded dialog program with test-only Shown event observations.
# It never invokes the application gate, clicks Agree, or creates an acceptance
# receipt. It is not an end-user agreement or interactive/accessibility review.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'This control contract requires Windows Forms on Windows.' }
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$source = [IO.File]::ReadAllText((Join-Path $root 'crates/flightsim-app/src/component_terms_dialog.ps1'))
$progressAnchor = '$ProgressPreference = ''SilentlyContinue'''
if (($source.Split(@($progressAnchor), [StringSplitOptions]::None)).Count -ne 2) {
    throw 'The exact production progress preference changed; review this synthetic harness.'
}
$anchor = '    [void]$form.ShowDialog()'
if (($source.Split(@($anchor), [StringSplitOptions]::None)).Count -ne 2) {
    throw 'The exact production dialog anchor changed; review this synthetic harness.'
}
$documents = Join-Path $root 'docs/release/components'
$data = @{
    version = 'SYNTHETIC-CONTROL-TEST-NO-ASSENT'
    sha256 = 'SYNTHETIC-CONTROL-TEST-NO-ASSENT'
    english = [IO.File]::ReadAllText((Join-Path $documents 'MICROSOFT-COMPONENT-TERMS.txt'))
    japanese = [IO.File]::ReadAllText((Join-Path $documents 'MICROSOFT-COMPONENT-TERMS.ja.txt'))
    notice = [IO.File]::ReadAllText((Join-Path $documents 'MICROSOFT-COMPONENT-NOTICE.txt'))
    project_licenses = ([IO.File]::ReadAllText((Join-Path $root 'LICENSE-MIT'))) + "`n" + ([IO.File]::ReadAllText((Join-Path $root 'LICENSE-APACHE')))
}
$probe = @'
    $script:observed = $false
    $script:caseFailure = $null
    $form.Add_Shown({
        try {
            if ($null -ne $form.AcceptButton) { throw 'Agree must not be the default Enter action.' }
            if ($form.CancelButton -ne $decline) { throw 'Esc must target Decline.' }
            if ($tabs.TabPages.Count -ne 4) { throw 'A full document tab is absent.' }
            if ($tabs.TabIndex -ne 0 -or $decline.TabIndex -ge $agree.TabIndex) { throw 'Review/choice keyboard order changed.' }
            if ([string]::IsNullOrWhiteSpace($form.AccessibleName) -or [string]::IsNullOrWhiteSpace($tabs.AccessibleName)) { throw 'Dialog accessibility names absent.' }
            $agreeName = 'Agree and continue / ' + (-join [char[]]@(0x540c, 0x610f, 0x3057, 0x3066, 0x7d9a, 0x884c))
            $declineName = 'Decline and exit / ' + (-join [char[]]@(0x540c, 0x610f, 0x305b, 0x305a, 0x7d42, 0x4e86))
            if ($agree.AccessibleName -ne $agreeName -or $decline.AccessibleName -ne $declineName) { throw 'Choice accessibility names changed.' }
            if (-not $heading.Text.Contains('Copyright (c) 2026 flightsim-claude contributors')) { throw 'Project copyright absent.' }
            $expected = @($data.english, $data.japanese, $data.notice, $data.project_licenses)
            for ($i = 0; $i -lt 4; $i++) {
                $control = $tabs.TabPages[$i].Controls[0]
                if ($control.Text.Replace("`r`n", "`n") -ne $expected[$i].Replace("`r`n", "`n")) { throw "Full document $i was changed or truncated." }
                if (-not $control.ReadOnly -or -not $control.TabStop -or -not $control.ShortcutsEnabled) { throw 'Document must be read-only, selectable and keyboard accessible.' }
                if ([string]::IsNullOrWhiteSpace($control.AccessibleName) -or [string]::IsNullOrWhiteSpace($control.AccessibleDescription)) { throw 'Document accessibility names absent.' }
            }
            $japaneseEnd = '6 ' + (-join [char[]]@(0x540c, 0x610f, 0x306e, 0x7bc4, 0x56f2))
            if (-not $tabs.TabPages[1].Controls[0].Text.Contains($japaneseEnd)) { throw 'Japanese end section was not delivered.' }
            $script:observed = $true
            if ($caseName -eq 'decline' -or $caseName -eq 'decline-with-progress') {
                # Synthetic sentinel detects that the actual Decline click
                # handler ran. The Agree control is never activated.
                $script:agreed = $true
                $decline.PerformClick()
                if ($script:agreed) { throw 'Decline did not clear the synthetic sentinel.' }
            } else {
                $form.Close()
            }
        } catch {
            $script:caseFailure = $_.Exception.Message
            $script:agreed = $false
            $form.Close()
        }
    })
    [void]$form.ShowDialog()
    if ($null -ne $script:caseFailure) { throw $script:caseFailure }
    if (-not $script:observed) { throw 'The actual Windows Forms controls were not observed.' }
'@
$private = Join-Path ([IO.Path]::GetTempPath()) ('flightsim-terms-synthetic-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($private)
try {
    foreach ($caseName in @('decline', 'decline-with-progress', 'close', 'missing-document')) {
        $program = '$caseName = ''' + $caseName + "'`n" + $source.Replace($anchor, $probe)
        if ($caseName -eq 'decline-with-progress') {
            # Force the same progress stream as first-use module preparation,
            # before any document/UI work. Real stderr is still rejected below.
            $progress = "`nWrite-Progress -Activity 'Synthetic module preparation' -Status 'Synthetic progress only' -PercentComplete 50"
            $program = $program.Replace($progressAnchor, $progressAnchor + $progress)
        }
        $start = New-Object Diagnostics.ProcessStartInfo
        $start.FileName = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($program))
        $start.Arguments = '-NoLogo -NoProfile -NonInteractive -STA -EncodedCommand ' + $encoded
        $start.UseShellExecute = $false
        $start.RedirectStandardInput = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.StandardOutputEncoding = New-Object Text.UTF8Encoding($false)
        $start.StandardErrorEncoding = New-Object Text.UTF8Encoding($false)
        $start.EnvironmentVariables['LOCALAPPDATA'] = $private
        $process = New-Object Diagnostics.Process
        $process.StartInfo = $start
        try {
            [void]$process.Start()
            $payload = $data.Clone()
            if ($caseName -eq 'missing-document') { $payload.english = '' }
            # Write exact UTF-8 bytes; PowerShell 5.1's default redirected writer
            # encoding otherwise varies by the host console code page.
            $bytes = [Text.Encoding]::UTF8.GetBytes(($payload | ConvertTo-Json -Depth 4 -Compress))
            $process.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
            $process.StandardInput.Close()
            if (-not $process.WaitForExit(30000)) {
                $process.Kill()
                $process.WaitForExit()
                throw "Synthetic $caseName control test timed out."
            }
            $stdout = $process.StandardOutput.ReadToEnd()
            $stderr = $process.StandardError.ReadToEnd()
            if ($caseName -eq 'missing-document') {
                if ($process.ExitCode -ne 2 -or $stdout.Length -ne 0 -or $stderr.Length -eq 0) { throw 'Incomplete document must fail without agreement.' }
            } elseif ($process.ExitCode -ne 1 -or $stdout.TrimEnd("`r", "`n") -ne 'FLIGHTSIM_COMPONENT_TERMS_DECLINE_V1' -or $stderr.Length -ne 0) {
                throw "Synthetic $caseName failed: exit=$($process.ExitCode), stdout=$stdout, stderr=$stderr"
            }
            if (@(Get-ChildItem -LiteralPath $private -Recurse -Force).Count -ne 0) { throw 'Synthetic dialog unexpectedly wrote user data.' }
            Write-Host "PASS synthetic component dialog controls: $caseName; no assent or receipt"
        } finally {
            $process.Dispose()
        }
    }
} finally {
    [IO.Directory]::Delete($private, $true)
}
