# The reader of the trial's native dialog on Windows, through UI Automation and MSAA; run by dialog.mjs.
# usage: windows.ps1 -Mode read -Id <pid> | -Mode press -Id <pid> -Press <close|allow|reject>
# read prints `dialog found=<yes|no> trusted=yes text=<same|differ|-> default=<reject|allow|none|other|-> cancel=<reject|allow|none|other|via_close|-> close=<yes|no|->`;
# press prints `pressed=<yes|no>`. It posts no input event: a control is pressed by its Invoke pattern.
param(
  [Parameter(Mandatory = $true)][ValidateSet('read', 'press')][string]$Mode,
  [Parameter(Mandatory = $true)][int]$Id,
  [ValidateSet('close', 'allow', 'reject')][string]$Press = 'reject'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -ReferencedAssemblies Accessibility -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Accessibility;
public static class R4Msaa {
  [DllImport("oleacc.dll")]
  static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint id, ref Guid iid, [MarshalAs(UnmanagedType.Interface)] out object obj);
  public static int State(IntPtr hwnd) {
    Guid iid = new Guid("618736E0-3C3D-11CF-810C-00AA00389B71");
    object found;
    if (AccessibleObjectFromWindow(hwnd, 0xFFFFFFFC, ref iid, out found) != 0 || found == null) { return -1; }
    return Convert.ToInt32(((IAccessible)found).get_accState(0));
  }
}
'@

$Title = 'Trial title'
$Body = (@('Label 1: one', 'Label 2: two', ('Label 3: ' + ('x' * 200)), 'Label 4: <b>bold</b> &amp;', 'Label 5: five') -join "`n")
$RejectLabel = "Don't pay"
$AllowLabel = 'Pay'
$STATE_SYSTEM_DEFAULT = 0x100
$IDCANCEL = '2'

$AE = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$Button = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
$Text = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)

function Get-Buttons($window) { return @($window.FindAll($Scope::Descendants, $Button)) }

function Find-Dialog {
  $mine = New-Object System.Windows.Automation.PropertyCondition($AE::ProcessIdProperty, $Id)
  foreach ($window in $AE::RootElement.FindAll($Scope::Children, $mine)) {
    $names = @(Get-Buttons $window | ForEach-Object { $_.Current.Name })
    if ($names -contains $RejectLabel -and $names -contains $AllowLabel) { return $window }
  }
  return $null
}

function Get-Label($control) {
  if ($null -eq $control) { return 'none' }
  switch ($control.Current.Name) {
    $RejectLabel { return 'reject' }
    $AllowLabel { return 'allow' }
    default { return 'other' }
  }
}

function Get-Close($window) {
  return @(Get-Buttons $window | Where-Object { $_.Current.AutomationId -eq 'Close' -and $_.Current.IsEnabled }) | Select-Object -First 1
}

function Read-Dialog {
  $window = Find-Dialog
  if ($null -eq $window) {
    'dialog found=no trusted=yes text=- default=- cancel=- close=-'
    return
  }
  $texts = @($window.FindAll($Scope::Descendants, $Text) | ForEach-Object { $_.Current.Name.Replace("`r`n", "`n") })
  $same = ($window.Current.Name -eq $Title) -and ($texts -contains $Body)
  $controls = @(Get-Buttons $window | Where-Object { $_.Current.Name -eq $RejectLabel -or $_.Current.Name -eq $AllowLabel })
  $default = $controls | Where-Object { $_.Current.NativeWindowHandle -ne 0 -and (([R4Msaa]::State([IntPtr]$_.Current.NativeWindowHandle)) -band $STATE_SYSTEM_DEFAULT) -ne 0 } | Select-Object -First 1
  $cancelButton = @(Get-Buttons $window | Where-Object { $_.Current.AutomationId -eq $IDCANCEL }) | Select-Object -First 1
  $close = Get-Close $window
  $cancel = if ($null -ne $cancelButton) { Get-Label $cancelButton } elseif ($null -ne $close) { 'via_close' } else { 'none' }
  $sameWord = if ($same) { 'same' } else { 'differ' }
  $closeWord = if ($null -ne $close) { 'yes' } else { 'no' }
  "dialog found=yes trusted=yes text=$sameWord default=$(Get-Label $default) cancel=$cancel close=$closeWord"
}

function Press-Control {
  $window = Find-Dialog
  if ($null -eq $window) { 'pressed=no'; return }
  $target = switch ($Press) {
    'close' { Get-Close $window }
    'allow' { @(Get-Buttons $window | Where-Object { $_.Current.Name -eq $AllowLabel }) | Select-Object -First 1 }
    default { @(Get-Buttons $window | Where-Object { $_.Current.Name -eq $RejectLabel }) | Select-Object -First 1 }
  }
  if ($null -eq $target) { 'pressed=no'; return }
  try {
    $target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    'pressed=yes'
  } catch {
    'pressed=no'
  }
}

if ($Mode -eq 'read') { Read-Dialog } else { Press-Control }
