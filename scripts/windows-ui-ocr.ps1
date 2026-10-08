param([switch]$Probe, [string]$Image)
# Local-only Windows OCR. No network, policy bypass, package install or credential.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType=WindowsRuntime]
$null = [Windows.Globalization.Language, Windows.Foundation, ContentType=WindowsRuntime]
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType=WindowsRuntime]
$null = [Windows.Storage.FileAccessMode, Windows.Storage, ContentType=WindowsRuntime]
$null = [Windows.Storage.Streams.IRandomAccessStream, Windows.Storage.Streams, ContentType=WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType=WindowsRuntime]
$null = [Windows.Graphics.Imaging.SoftwareBitmap, Windows.Graphics.Imaging, ContentType=WindowsRuntime]
$null = [Windows.Media.Ocr.OcrResult, Windows.Foundation, ContentType=WindowsRuntime]
$language = New-Object Windows.Globalization.Language('en-US')
$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($language)
if ($null -eq $engine) { throw 'Required local en-US OCR engine is unavailable' }
if ($Probe) {
    @{ schema_version = 1; engine = 'Windows.Media.Ocr'; language = 'en-US'; available = $true } | ConvertTo-Json -Compress
    exit 0
}
if ([string]::IsNullOrEmpty($Image) -or -not [IO.Path]::IsPathRooted($Image)) { throw 'Absolute image path required' }
$fileInfo = Get-Item -LiteralPath $Image
if ($fileInfo.Length -gt 16777216 -or ($fileInfo.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Invalid image input' }
$asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.IsGenericMethodDefinition -and $_.GetGenericArguments().Count -eq 1 -and
    $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
} | Select-Object -First 1
if ($null -eq $asTask) { throw 'WinRT async bridge is unavailable' }
function Await-Operation($operation, [Type]$resultType) {
    $task = $asTask.MakeGenericMethod($resultType).Invoke($null, @($operation))
    if (-not $task.Wait(15000)) { throw 'OCR operation timed out' }
    return $task.Result
}
$file = Await-Operation ([Windows.Storage.StorageFile]::GetFileFromPathAsync($Image)) ([Windows.Storage.StorageFile])
$stream = Await-Operation ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
try {
    $decoder = Await-Operation ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
    $bitmap = Await-Operation ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
    try {
        if ($bitmap.PixelWidth -gt 4096 -or $bitmap.PixelHeight -gt 4096) { throw 'OCR dimensions exceed budget' }
        $result = Await-Operation ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
        if ($result.Text.Length -gt 32768) { throw 'OCR text exceeds budget' }
        $words = @()
        foreach ($line in $result.Lines) {
            foreach ($word in $line.Words) {
                $box = $word.BoundingRect
                $words += @{ text = $word.Text; x = $box.X; y = $box.Y; width = $box.Width; height = $box.Height }
                if ($words.Count -gt 2048) { throw 'OCR word budget exceeded' }
            }
        }
        @{ schema_version = 1; engine = 'Windows.Media.Ocr'; language = 'en-US'; text = $result.Text; words = $words } | ConvertTo-Json -Depth 5 -Compress
    } finally { if ($null -ne $bitmap) { $bitmap.Dispose() } }
} finally { if ($null -ne $stream) { $stream.Dispose() } }
