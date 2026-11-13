# The reader of the trial's native dialog on Windows, through MSAA (oleacc); run by dialog.mjs.
# usage: windows.ps1 -Mode read -Id <pid> | -Mode list -Id <pid> | -Mode press -Id <pid> -Press <close|allow|reject>
# read prints `dialog found=<yes|no> trusted=yes text=<same|differ|-> default=<reject|allow|none|other|unread|-> cancel=<reject|allow|none|other|via_close|unread|-> close=<yes|no|unread|-> owner=<app|->`;
# list prints counts of the process's visible top-level windows, those titled as the dialog, and their buttons; press prints `pressed=<yes|no>`.
# It posts no input event: a control is pressed by its MSAA default action.
param(
  [Parameter(Mandatory = $true)][ValidateSet('read', 'list', 'press')][string]$Mode,
  [Parameter(Mandatory = $true)][int]$Id,
  [ValidateSet('close', 'allow', 'reject')][string]$Press = 'reject'
)
$ErrorActionPreference = 'Stop'
$Accessibility = (Add-Type -AssemblyName Accessibility -PassThru | Select-Object -First 1).Assembly.Location
Add-Type -ReferencedAssemblies $Accessibility -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using Accessibility;
public static class R4Msaa {
  public const int STATE_SYSTEM_DEFAULT = 0x100;
  public const int STATE_SYSTEM_UNAVAILABLE = 0x1;
  public const int STATE_SYSTEM_INVISIBLE = 0x8000;
  public const int ROLE_SYSTEM_PUSHBUTTON = 0x2B;
  public const int IDCANCEL = 2;
  const uint OBJID_WINDOW = 0x00000000;
  const uint OBJID_TITLEBAR = 0xFFFFFFFE;
  const int TITLEBAR_CLOSE = 5;
  delegate bool EnumProc(IntPtr hwnd, IntPtr param);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr param);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc proc, IntPtr param);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr hwnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("oleacc.dll")] static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint id, ref Guid iid, [MarshalAs(UnmanagedType.Interface)] out object obj);
  [DllImport("oleacc.dll")] static extern int AccessibleChildren(IAccessible parent, int start, int count, [In, Out] object[] children, out int obtained);

  public class Node { public IAccessible Acc; public object Child; }

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
    StringBuilder text = new StringBuilder(1024);
    GetWindowText(hwnd, text, 1024);
    return text.ToString();
  }

  public static string ClassOf(IntPtr hwnd) {
    StringBuilder text = new StringBuilder(256);
    GetClassName(hwnd, text, 256);
    return text.ToString();
  }

  static IAccessible FromWindow(IntPtr hwnd, uint id) {
    Guid iid = new Guid("618736E0-3C3D-11CF-810C-00AA00389B71");
    object found;
    if (AccessibleObjectFromWindow(hwnd, id, ref iid, out found) != 0) { return null; }
    return found as IAccessible;
  }

  static void Walk(IAccessible acc, int depth, List<Node> nodes) {
    nodes.Add(new Node { Acc = acc, Child = 0 });
    int count = 0;
    try { count = acc.accChildCount; } catch { count = 0; }
    if (count <= 0 || depth > 10) { return; }
    object[] children = new object[count];
    int got;
    if (AccessibleChildren(acc, 0, count, children, out got) != 0) { return; }
    for (int i = 0; i < got; i++) {
      IAccessible child = children[i] as IAccessible;
      if (child != null) { Walk(child, depth + 1, nodes); }
      else if (children[i] is int) { nodes.Add(new Node { Acc = acc, Child = children[i] }); }
    }
  }

  public static List<Node> Nodes(IntPtr hwnd) {
    List<Node> nodes = new List<Node>();
    IAccessible root = FromWindow(hwnd, OBJID_WINDOW);
    if (root != null) { Walk(root, 0, nodes); }
    return nodes;
  }

  public static string Name(Node n) { try { return n.Acc.get_accName(n.Child); } catch { return null; } }
  public static string Value(Node n) { try { return n.Acc.get_accValue(n.Child); } catch { return null; } }
  public static int State(Node n) { try { return Convert.ToInt32(n.Acc.get_accState(n.Child)); } catch { return -1; } }
  public static int Role(Node n) { try { object r = n.Acc.get_accRole(n.Child); return r is int ? (int)r : -1; } catch { return -1; } }

  public static List<Node> Buttons(IntPtr hwnd, string label) {
    List<Node> found = new List<Node>();
    foreach (Node n in Nodes(hwnd)) {
      if (Role(n) == ROLE_SYSTEM_PUSHBUTTON && Name(n) == label) { found.Add(n); }
    }
    return found;
  }

  public static int PushButtons(IntPtr hwnd) {
    int count = 0;
    foreach (Node n in Nodes(hwnd)) { if (Role(n) == ROLE_SYSTEM_PUSHBUTTON) { count++; } }
    return count;
  }

  public static bool HasText(IntPtr hwnd, string body) {
    foreach (Node n in Nodes(hwnd)) {
      foreach (string s in new string[] { Name(n), Value(n) }) {
        if (s != null && s.Replace("\r\n", "\n") == body) { return true; }
      }
    }
    return false;
  }

  public static string CancelLabel(IntPtr hwnd) {
    string label = null;
    EnumChildWindows(hwnd, (child, param) => {
      if (GetDlgCtrlID(child) == IDCANCEL && ClassOf(child) == "Button") { label = Text(child); return false; }
      return true;
    }, IntPtr.Zero);
    return label;
  }

  public static int CloseState(IntPtr hwnd) {
    IAccessible bar = FromWindow(hwnd, OBJID_TITLEBAR);
    if (bar == null) { return -1; }
    try { return Convert.ToInt32(bar.get_accState(TITLEBAR_CLOSE)); } catch { return -1; }
  }

  public static bool PressClose(IntPtr hwnd) {
    IAccessible bar = FromWindow(hwnd, OBJID_TITLEBAR);
    if (bar == null) { return false; }
    try { bar.accDoDefaultAction(TITLEBAR_CLOSE); return true; } catch { return false; }
  }

  public static bool PressButton(Node n) {
    try { n.Acc.accDoDefaultAction(n.Child); return true; } catch { return false; }
  }
}
'@

$Title = 'Trial title'
$Body = (@('Label 1: one', 'Label 2: two', ('Label 3: ' + ('x' * 200)), 'Label 4: <b>bold</b> &amp;', 'Label 5: five') -join "`n")
$RejectLabel = "Don't pay"
$AllowLabel = 'Pay'
$STATE_SYSTEM_DEFAULT = 0x100

function Find-Dialog {
  foreach ($handle in @([R4Msaa]::Windows([uint32]$Id))) {
    if ([R4Msaa]::Text($handle) -eq $Title) { return $handle }
  }
  return $null
}

function Get-Label([string]$name) {
  if ($null -eq $name -or $name -eq '') { return 'none' }
  if ($name -eq $RejectLabel) { return 'reject' }
  if ($name -eq $AllowLabel) { return 'allow' }
  return 'other'
}

function Read-Dialog {
  $handle = Find-Dialog
  if ($null -eq $handle) {
    'dialog found=no trusted=yes text=- default=- cancel=- close=- owner=-'
    return
  }
  $reject = @([R4Msaa]::Buttons($handle, $RejectLabel))
  $allow = @([R4Msaa]::Buttons($handle, $AllowLabel))
  $defaultWord = 'unread'
  if ($reject.Count -eq 1 -and $allow.Count -eq 1) {
    $rs = [R4Msaa]::State($reject[0])
    $as = [R4Msaa]::State($allow[0])
    if ($rs -ge 0 -and $as -ge 0) {
      $defaultWord = 'none'
      if (($rs -band $STATE_SYSTEM_DEFAULT) -ne 0) { $defaultWord = 'reject' } elseif (($as -band $STATE_SYSTEM_DEFAULT) -ne 0) { $defaultWord = 'allow' }
    }
  }
  $sameWord = if ([R4Msaa]::HasText($handle, $Body)) { 'same' } else { 'differ' }
  $closeState = [R4Msaa]::CloseState($handle)
  $hidden = [R4Msaa]::STATE_SYSTEM_INVISIBLE -bor [R4Msaa]::STATE_SYSTEM_UNAVAILABLE
  $closeWord = if ($closeState -lt 0) { 'unread' } elseif (($closeState -band $hidden) -eq 0) { 'yes' } else { 'no' }
  $cancelName = [R4Msaa]::CancelLabel($handle)
  $cancel = if ($null -ne $cancelName) { Get-Label $cancelName } elseif ($closeWord -eq 'yes') { 'via_close' } elseif ($closeWord -eq 'no') { 'none' } else { 'unread' }
  "dialog found=yes trusted=yes text=$sameWord default=$defaultWord cancel=$cancel close=$closeWord owner=app"
}

function List-Windows {
  $handles = @([R4Msaa]::Windows([uint32]$Id))
  $titled = @($handles | Where-Object { [R4Msaa]::Text($_) -eq $Title })
  $buttons = 0
  $reject = 0
  $allow = 0
  foreach ($handle in $titled) {
    $buttons += [R4Msaa]::PushButtons($handle)
    $reject += @([R4Msaa]::Buttons($handle, $RejectLabel)).Count
    $allow += @([R4Msaa]::Buttons($handle, $AllowLabel)).Count
  }
  $classes = (@($handles | ForEach-Object { [R4Msaa]::ClassOf($_) -replace '[^A-Za-z0-9_#]', '_' }) -join ',')
  if ($classes -eq '') { $classes = '-' }
  "list windows=$($handles.Count) titled=$($titled.Count) buttons=$buttons reject_named=$reject allow_named=$allow classes=$classes"
}

function Press-Control {
  $handle = Find-Dialog
  if ($null -eq $handle) { 'pressed=no'; return }
  if ($Press -eq 'close') {
    if ([R4Msaa]::PressClose($handle)) { 'pressed=yes' } else { 'pressed=no' }
    return
  }
  $label = if ($Press -eq 'allow') { $AllowLabel } else { $RejectLabel }
  $found = @([R4Msaa]::Buttons($handle, $label))
  if ($found.Count -ne 1) { 'pressed=no'; return }
  if ([R4Msaa]::PressButton($found[0])) { 'pressed=yes' } else { 'pressed=no' }
}

if ($Mode -eq 'read') { Read-Dialog } elseif ($Mode -eq 'list') { List-Windows } else { Press-Control }
