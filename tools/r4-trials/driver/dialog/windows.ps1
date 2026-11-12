# The reader of the trial's native dialog on Windows, through UI Automation and MSAA; run by dialog.mjs.
# usage: windows.ps1 -Mode read -Id <pid> | -Mode list -Id <pid> | -Mode press -Id <pid> -Press <close|allow|reject>
# read prints `dialog found=<yes|no> trusted=yes text=<same|differ|-> default=<reject|allow|none|other|unread|-> cancel=<reject|allow|none|other|via_close|unread|-> close=<yes|no|unread|->`;
# list prints counts of the process's visible top-level windows, those titled as the dialog, and their buttons; press prints `pressed=<yes|no>`. It posts no input event: a control is pressed by its Invoke pattern.
param(
  [Parameter(Mandatory = $true)][ValidateSet('read', 'list', 'press')][string]$Mode,
  [Parameter(Mandatory = $true)][int]$Id,
  [ValidateSet('close', 'allow', 'reject')][string]$Press = 'reject'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$Accessibility = (Add-Type -AssemblyName Accessibility -PassThru | Select-Object -First 1).Assembly.Location
Add-Type -ReferencedAssemblies $Accessibility -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using Accessibility;
public static class R4Msaa {
  delegate bool EnumProc(IntPtr hwnd, IntPtr param);
  [DllImport("user32.dll")]
  static extern bool EnumWindows(EnumProc proc, IntPtr param);
  [DllImport("user32.dll")]
  static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")]
  static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
  public static IntPtr[] Windows(uint owner) {
    List<IntPtr> found = new List<IntPtr>();
    EnumWindows((hwnd, param) => {
      uint pid;
      GetWindowThreadProcessId(hwnd, out pid);
      if (pid == owner && IsWindowVisible(hwnd)) { found.Add(hwnd); }
      return true;
    }, IntPtr.Zero);
    return found.ToArray();
  }
  public static string Text(IntPtr hwnd) {
    StringBuilder text = new StringBuilder(512);
    GetWindowText(hwnd, text, 512);
    return text.ToString();
  }
  [DllImport("user32.dll")]
  static extern int GetWindowLong(IntPtr hwnd, int index);
  public static bool HasSystemMenu(IntPtr hwnd) { return (GetWindowLong(hwnd, -16) & 0x80000) != 0; }
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

function Get-Windows {
  return @([R4Msaa]::Windows([uint32]$Id) | ForEach-Object { $AE::FromHandle($_) })
}

function Find-Dialog {
  foreach ($window in $(Get-Windows)) {
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
  $states = @($controls | ForEach-Object {
    $handle = $_.Current.NativeWindowHandle
    if ($handle -eq 0) { -1 } else { [R4Msaa]::State([IntPtr]$handle) }
  })
  $defaultWord = 'none'
  if ($controls.Count -ne 2 -or $states -contains -1) {
    $defaultWord = 'unread'
  } else {
    for ($i = 0; $i -lt 2; $i++) {
      if (($states[$i] -band $STATE_SYSTEM_DEFAULT) -ne 0) { $defaultWord = Get-Label $controls[$i]; break }
    }
  }
  $cancelButton = @(Get-Buttons $window | Where-Object { $_.Current.AutomationId -eq $IDCANCEL }) | Select-Object -First 1
  $close = Get-Close $window
  $cancel = if ($null -ne $cancelButton) { Get-Label $cancelButton } elseif ($null -ne $close) { 'via_close' } else { 'none' }
  $sameWord = if ($same) { 'same' } else { 'differ' }
  $menu = [R4Msaa]::HasSystemMenu([IntPtr]$window.Current.NativeWindowHandle)
  $closeWord = if ($null -ne $close) { 'yes' } elseif ($menu) { 'unread' } else { 'no' }
  if ($closeWord -eq 'unread' -and $cancel -eq 'none') { $cancel = 'unread' }
  "dialog found=yes trusted=yes text=$sameWord default=$defaultWord cancel=$cancel close=$closeWord"
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

function List-Windows {
  $handles = @([R4Msaa]::Windows([uint32]$Id))
  $titled = @($handles | Where-Object { [R4Msaa]::Text($_) -eq $Title }).Count
  $windows = @(Get-Windows)
  $buttons = @($windows | ForEach-Object { Get-Buttons $_ })
  $reject = @($buttons | Where-Object { $_.Current.Name -eq $RejectLabel }).Count
  $allow = @($buttons | Where-Object { $_.Current.Name -eq $AllowLabel }).Count
  $classes = (@($windows | ForEach-Object { $_.Current.ClassName -replace '[^A-Za-z0-9_#]', '_' }) -join ',')
  if ($classes -eq '') { $classes = '-' }
  "list windows=$($windows.Count) titled=$titled buttons=$($buttons.Count) reject_named=$reject allow_named=$allow classes=$classes"
}

if ($Mode -eq 'read') { Read-Dialog } elseif ($Mode -eq 'list') { List-Windows } else { Press-Control }
