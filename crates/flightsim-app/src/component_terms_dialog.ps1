# Fixed embedded local Windows Forms program. All document text arrives as JSON
# data on standard input. Only the Agree button emits the acceptance protocol.
# Suppress first-use module progress on redirected stderr, never real errors.
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
try {
    [Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
    [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
    $data = [Console]::In.ReadToEnd() | ConvertFrom-Json
    foreach ($name in @('version', 'sha256', 'english', 'japanese', 'notice', 'project_licenses')) {
        if (-not ($data.$name -is [string]) -or [string]::IsNullOrWhiteSpace($data.$name)) {
            throw 'A complete component document is missing.'
        }
    }
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    [System.Windows.Forms.Application]::EnableVisualStyles()

    $script:agreed = $false
    $form = New-Object System.Windows.Forms.Form
    $form.Text = 'FlightSim - Microsoft component terms / コンポーネント補足規約'
    $form.StartPosition = 'CenterScreen'
    $form.AutoScaleMode = 'Dpi'
    $form.Font = New-Object System.Drawing.Font('Segoe UI', 10)
    $area = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
    $width = [Math]::Min(980, [Math]::Max(320, $area.Width - 40))
    $height = [Math]::Min(780, [Math]::Max(320, $area.Height - 40))
    $form.Size = New-Object System.Drawing.Size($width, $height)
    $form.MinimumSize = New-Object System.Drawing.Size([Math]::Min(600, $width), [Math]::Min(480, $height))
    $form.AccessibleName = 'FlightSim Microsoft component terms'

    $layout = New-Object System.Windows.Forms.TableLayoutPanel
    $layout.Dock = 'Fill'
    $layout.Padding = New-Object System.Windows.Forms.Padding(12)
    $layout.ColumnCount = 1
    [void]$layout.ColumnStyles.Add((New-Object System.Windows.Forms.ColumnStyle('Percent', 100)))
    $layout.RowCount = 4
    [void]$layout.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('AutoSize')))
    [void]$layout.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('Percent', 100)))
    [void]$layout.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('AutoSize')))
    [void]$layout.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('AutoSize')))
    $form.Controls.Add($layout)

    $heading = New-Object System.Windows.Forms.Label
    $heading.AutoSize = $true
    $heading.Dock = 'Fill'
    $heading.Padding = New-Object System.Windows.Forms.Padding(0, 0, 0, 8)
    $heading.Text = "Please read the complete English terms and choose. 日本語参考訳・表示は各タブで確認できます。`r`nCopyright (c) 2026 flightsim-claude contributors`r`nVersion $($data.version) | English SHA-256: $($data.sha256)"
    $layout.Controls.Add($heading, 0, 0)

    $tabs = New-Object System.Windows.Forms.TabControl
    $tabs.Dock = 'Fill'
    $tabs.AccessibleName = 'Complete component documents'
    $tabs.TabIndex = 0
    $firstText = $null
    foreach ($entry in @(
        @{ Title = 'English terms'; Content = $data.english },
        @{ Title = '日本語参考訳'; Content = $data.japanese },
        @{ Title = 'Component notice / 表示'; Content = $data.notice },
        @{ Title = 'Project licenses'; Content = $data.project_licenses }
    )) {
        $page = New-Object System.Windows.Forms.TabPage
        $page.Text = $entry.Title
        $page.Padding = New-Object System.Windows.Forms.Padding(6)
        $text = New-Object System.Windows.Forms.RichTextBox
        $text.Dock = 'Fill'
        $text.ReadOnly = $true
        $text.DetectUrls = $false
        $text.WordWrap = $true
        $text.ScrollBars = 'Vertical'
        $text.ShortcutsEnabled = $true
        $text.AcceptsTab = $false
        $text.TabStop = $true
        $text.AccessibleName = $entry.Title
        $text.AccessibleDescription = 'Complete, selectable text. Use arrow keys or Page Down to read; Ctrl+A and Ctrl+C to copy.'
        $text.Text = $entry.Content.Replace("`r`n", "`n").Replace("`n", "`r`n")
        $text.SelectionStart = 0
        $text.SelectionLength = 0
        $page.Controls.Add($text)
        [void]$tabs.TabPages.Add($page)
        if ($null -eq $firstText) { $firstText = $text }
    }
    $layout.Controls.Add($tabs, 0, 1)

    $hint = New-Object System.Windows.Forms.Label
    $hint.AutoSize = $true
    $hint.Dock = 'Fill'
    $hint.Padding = New-Object System.Windows.Forms.Padding(0, 8, 0, 8)
    $hint.Text = "Agreement concerns only the Microsoft component supplement. No account or telemetry consent.`r`nRedistributors: review and agree using --component-terms before redistribution. This command exits after your choice.`r`nClosing this window or pressing Esc declines. 同意しない場合、または閉じた場合は終了します。"
    $layout.Controls.Add($hint, 0, 2)

    $buttons = New-Object System.Windows.Forms.FlowLayoutPanel
    $buttons.Dock = 'Fill'
    $buttons.AutoSize = $true
    $buttons.WrapContents = $true
    $buttons.FlowDirection = 'RightToLeft'
    $agree = New-Object System.Windows.Forms.Button
    $agree.Text = 'Agree and continue / 同意して続行'
    $agree.AccessibleName = 'Agree and continue / 同意して続行'
    $agree.AutoSize = $true
    $agree.Padding = New-Object System.Windows.Forms.Padding(8)
    $agree.TabIndex = 1
    $agree.Add_Click({
        $script:agreed = $true
        $form.Close()
    })
    $decline = New-Object System.Windows.Forms.Button
    $decline.Text = 'Decline and exit / 同意せず終了'
    $decline.AccessibleName = 'Decline and exit / 同意せず終了'
    $decline.AutoSize = $true
    $decline.Padding = New-Object System.Windows.Forms.Padding(8)
    $decline.TabIndex = 0
    $decline.Add_Click({
        $script:agreed = $false
        $form.Close()
    })
    $buttons.Controls.Add($agree)
    $buttons.Controls.Add($decline)
    $layout.Controls.Add($buttons, 0, 3)
    # Enter is never an implicit acceptance shortcut. Standard keyboard
    # activation still works after deliberately focusing the Agree button.
    $form.AcceptButton = $null
    $form.CancelButton = $decline
    $form.Add_Shown({ [void]$firstText.Focus() })
    [void]$form.ShowDialog()
    $form.Dispose()
    if ($script:agreed) {
        [Console]::Out.WriteLine("FLIGHTSIM_COMPONENT_TERMS_AGREE_V1:$($data.version):$($data.sha256)")
        exit 0
    }
    [Console]::Out.WriteLine('FLIGHTSIM_COMPONENT_TERMS_DECLINE_V1')
    exit 1
} catch {
    [Console]::Error.WriteLine('Unable to display the complete component terms; no agreement recorded.')
    exit 2
}
