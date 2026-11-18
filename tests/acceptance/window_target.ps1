# Create a controlled real Win32 window as the window-capture regression target.
# It is topmost and writes HWND / physical rectangle to JSON for the CDP test.
param(
  [string]$Report = (Join-Path $env:TEMP 'snapcap-window-target.json')
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class WinTargetNative {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
'@

# Report physical pixels so the captured PNG dimensions can be compared exactly.
[void][WinTargetNative]::SetProcessDPIAware()

$form = New-Object System.Windows.Forms.Form
$form.Text = 'SnapCap E2E Target'
$form.StartPosition = 'Manual'
$form.Location = New-Object System.Drawing.Point(180, 180)
$form.ClientSize = New-Object System.Drawing.Size(520, 320)
$form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedSingle
$form.MaximizeBox = $false
$form.TopMost = $true
$form.BackColor = [System.Drawing.Color]::FromArgb(24, 92, 158)
$form.ForeColor = [System.Drawing.Color]::White
$form.Font = New-Object System.Drawing.Font('Segoe UI', 16)

$label = New-Object System.Windows.Forms.Label
$label.Text = 'SnapCap window capture target'
$label.AutoSize = $true
$label.Location = New-Object System.Drawing.Point(36, 80)
$form.Controls.Add($label)
$status = New-Object System.Windows.Forms.Label
$status.Text = 'CDP + Enter end-to-end test'
$status.AutoSize = $true
$status.Location = New-Object System.Drawing.Point(36, 128)
$form.Controls.Add($status)

$form.Add_Shown({
  $form.Activate()
  $rect = New-Object WinTargetNative+RECT
  [void][WinTargetNative]::GetWindowRect($form.Handle, [ref]$rect)
  $info = [ordered]@{
    hwnd = $form.Handle.ToInt64()
    title = $form.Text
    process_id = $PID
    x = $rect.Left
    y = $rect.Top
    width = $rect.Right - $rect.Left
    height = $rect.Bottom - $rect.Top
  }
  $info | ConvertTo-Json -Compress | Set-Content -Encoding UTF8 $Report
})

[void]$form.Show()
[System.Windows.Forms.Application]::Run($form)
