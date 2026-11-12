// The reader of the trial's native dialog on macOS, through the accessibility interface; built and run by dialog.mjs.
// usage: macos read <pid> | list <pid> | press <pid> <close|allow|reject>
// read prints `dialog found=<yes|no> trusted=<yes|no> text=<same|differ|-> default=<reject|allow|none|other|-> cancel=<...> close=<yes|no|->`;
// list prints counts of the app's windows and their buttons; press prints `pressed=<yes|no>`. It posts no input event: a control is pressed by its AXPress action.
import ApplicationServices
import Foundation

let title = "Trial title"
let body = ["Label 1: one", "Label 2: two", "Label 3: " + String(repeating: "x", count: 200), "Label 4: <b>bold</b> &amp;", "Label 5: five"].joined(separator: "\n")
let rejectLabel = "Don't pay"
let allowLabel = "Pay"
let defaultAttr = "AXDefaultButton"
let cancelAttr = "AXCancelButton"
let closeAttr = "AXCloseButton"

func refuse(_ detail: String) -> Never {
  FileHandle.standardError.write(("REFUSED usage: " + detail + "\n").data(using: .utf8)!)
  exit(2)
}

func value(_ element: AXUIElement, _ name: String) -> AnyObject? {
  var out: AnyObject?
  return AXUIElementCopyAttributeValue(element, name as CFString, &out) == .success ? out : nil
}

func element(_ element: AXUIElement, _ name: String) -> AXUIElement? {
  guard let found = value(element, name), CFGetTypeID(found) == AXUIElementGetTypeID() else { return nil }
  return (found as! AXUIElement)
}

func text(_ element: AXUIElement, _ name: String) -> String? {
  return value(element, name) as? String
}

func descendants(_ root: AXUIElement, _ depth: Int = 0) -> [AXUIElement] {
  guard depth < 12, let children = value(root, kAXChildrenAttribute as String) as? [AXUIElement] else { return [] }
  return children + children.flatMap { descendants($0, depth + 1) }
}

func role(_ e: AXUIElement) -> String { return text(e, kAXRoleAttribute as String) ?? "" }

func buttons(_ window: AXUIElement) -> [AXUIElement] {
  return descendants(window).filter { role($0) == (kAXButtonRole as String) }
}

func dialog(_ pid: pid_t) -> AXUIElement? {
  let app = AXUIElementCreateApplication(pid)
  let windows = (value(app, kAXWindowsAttribute as String) as? [AXUIElement]) ?? []
  for window in windows {
    let titles = Set(buttons(window).compactMap { text($0, kAXTitleAttribute as String) })
    if titles.contains(rejectLabel) && titles.contains(allowLabel) { return window }
  }
  return nil
}

func name(of control: AXUIElement?) -> String {
  guard let control = control else { return "none" }
  switch text(control, kAXTitleAttribute as String) {
  case rejectLabel?: return "reject"
  case allowLabel?: return "allow"
  default: return "other"
  }
}

func normal(_ s: String) -> String { return s.replacingOccurrences(of: "\r\n", with: "\n") }

func read(_ pid: pid_t) {
  let trusted = AXIsProcessTrusted() ? "yes" : "no"
  guard let window = dialog(pid) else {
    print("dialog found=no trusted=\(trusted) text=- default=- cancel=- close=-")
    return
  }
  let texts = descendants(window).filter { role($0) == (kAXStaticTextRole as String) }.compactMap { text($0, kAXValueAttribute as String) }.map(normal)
  let same = texts.contains(title) && texts.contains(body)
  let close = element(window, closeAttr)
  let closeEnabled = close.flatMap { value($0, kAXEnabledAttribute as String) as? Bool } ?? false
  print("dialog found=yes trusted=\(trusted) text=\(same ? "same" : "differ") default=\(name(of: element(window, defaultAttr))) cancel=\(name(of: element(window, cancelAttr))) close=\(close != nil && closeEnabled ? "yes" : "no")")
}

func list(_ pid: pid_t) {
  let app = AXUIElementCreateApplication(pid)
  let windows = (value(app, kAXWindowsAttribute as String) as? [AXUIElement]) ?? []
  let all = windows.flatMap { buttons($0) }
  let titles = all.compactMap { text($0, kAXTitleAttribute as String) }
  let subroles = windows.map { (text($0, kAXSubroleAttribute as String) ?? "-").filter { $0.isLetter || $0.isNumber } }
  print("list trusted=\(AXIsProcessTrusted() ? "yes" : "no") windows=\(windows.count) buttons=\(all.count) reject_named=\(titles.filter { $0 == rejectLabel }.count) allow_named=\(titles.filter { $0 == allowLabel }.count) classes=\(subroles.isEmpty ? "-" : subroles.joined(separator: ","))")
}

func press(_ pid: pid_t, _ which: String) {
  guard let window = dialog(pid) else {
    print("pressed=no")
    return
  }
  let target: AXUIElement?
  switch which {
  case "close": target = element(window, closeAttr)
  case "allow": target = buttons(window).first { text($0, kAXTitleAttribute as String) == allowLabel }
  default: target = buttons(window).first { text($0, kAXTitleAttribute as String) == rejectLabel }
  }
  guard let control = target else {
    print("pressed=no")
    return
  }
  print(AXUIElementPerformAction(control, kAXPressAction as CFString) == .success ? "pressed=yes" : "pressed=no")
}

let args = CommandLine.arguments
guard args.count >= 3, let pid = pid_t(args[2]), pid > 0 else { refuse("read <pid> | list <pid> | press <pid> <close|allow|reject>") }
switch (args[1], args.count) {
case ("read", 3): read(pid)
case ("list", 3): list(pid)
case ("press", 4) where ["close", "allow", "reject"].contains(args[3]): press(pid, args[3])
default: refuse("read <pid> | list <pid> | press <pid> <close|allow|reject>")
}
