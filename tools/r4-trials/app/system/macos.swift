// The reader of the system for macOS, built and run by system.mjs.
// usage: macos window <pid> <title> <width> | tree <pid> | alive <pid>:<start>...
import CoreGraphics
import Darwin
import Foundation

typealias Responsible = @convention(c) (pid_t) -> pid_t
let responsibleCall = dlsym(dlopen(nil, RTLD_NOW), "responsibility_get_pid_responsible_for_pid")

func refuse(_ detail: String) -> Never {
  FileHandle.standardError.write(("REFUSED usage: " + detail + "\n").data(using: .utf8)!)
  exit(2)
}

func info(_ pid: pid_t) -> proc_bsdinfo? {
  var bsd = proc_bsdinfo()
  let size = Int32(MemoryLayout<proc_bsdinfo>.size)
  return proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &bsd, size) == size ? bsd : nil
}

func started(_ bsd: proc_bsdinfo) -> UInt64 {
  return bsd.pbi_start_tvsec &* 1_000_000 &+ bsd.pbi_start_tvusec
}

func name(_ pid: pid_t) -> String {
  var buffer = [CChar](repeating: 0, count: 4096)
  if proc_pidpath(pid, &buffer, UInt32(buffer.count)) > 0 {
    return (String(cString: buffer) as NSString).lastPathComponent
  }
  var short = [CChar](repeating: 0, count: 256)
  proc_name(pid, &short, UInt32(short.count))
  return String(cString: short)
}

func responsible(_ pid: pid_t) -> pid_t {
  guard let call = responsibleCall else { return -1 }
  return unsafeBitCast(call, to: Responsible.self)(pid)
}

func allPids() -> [pid_t] {
  let count = proc_listallpids(nil, 0)
  var pids = [pid_t](repeating: 0, count: Int(max(count, 0)) + 256)
  let got = proc_listallpids(&pids, Int32(pids.count * MemoryLayout<pid_t>.size))
  return Array(pids.prefix(Int(max(got, 0)))).filter { $0 > 0 }
}

func isWebView(_ name: String) -> Bool {
  return name.hasPrefix("com.apple.WebKit.")
}

func window(_ pid: pid_t, _ title: String, _ width: Int) {
  let list = (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]]) ?? []
  var matched = 0
  var onscreen = 0
  var byTitle = false
  for entry in list {
    guard let owner = entry[kCGWindowOwnerPID as String] as? Int32, owner == pid else { continue }
    guard ((entry[kCGWindowLayer as String] as? Int) ?? -1) == 0 else { continue }
    let bounds = (entry[kCGWindowBounds as String] as? [String: Any]) ?? [:]
    let w = Int((bounds["Width"] as? Double) ?? 0)
    let found: Bool
    if let windowName = entry[kCGWindowName as String] as? String {
      byTitle = true
      found = windowName == title
    } else {
      found = w == width
    }
    if found {
      matched += 1
      if (entry[kCGWindowIsOnscreen as String] as? Bool) ?? false { onscreen += 1 }
    }
  }
  let state = onscreen > 0 ? "onscreen" : (matched > 0 ? "hidden" : "absent")
  print("window=\(state) matched_by=\(byTitle ? "title" : "width")")
}

func tree(_ root: pid_t) {
  guard let rootInfo = info(root) else {
    print("tree count=0 webview=0")
    return
  }
  let rootStart = started(rootInfo)
  let rootResponsible = responsible(root)
  var parents: [pid_t: pid_t] = [:]
  var starts: [pid_t: UInt64] = [:]
  for pid in allPids() {
    if let bsd = info(pid) {
      parents[pid] = pid_t(bsd.pbi_ppid)
      starts[pid] = started(bsd)
    }
  }
  var members: Set<pid_t> = [root]
  var grew = true
  while grew {
    grew = false
    for (pid, parent) in parents where members.contains(parent) && !members.contains(pid) {
      members.insert(pid)
      grew = true
    }
  }
  for (pid, start) in starts where !members.contains(pid) && start >= rootStart {
    guard isWebView(name(pid)) else { continue }
    let owner = responsible(pid)
    if owner == root || (rootResponsible > 0 && rootResponsible != root && owner == rootResponsible) {
      members.insert(pid)
    }
  }
  let webviews = members.filter { isWebView(name($0)) }.count
  print("tree count=\(members.count) webview=\(webviews)")
  for pid in members.sorted() {
    print("pid=\(pid) start=\(starts[pid] ?? 0)")
  }
}

func alive(_ pairs: [String]) {
  var count = 0
  for pair in pairs {
    let parts = pair.split(separator: ":")
    guard parts.count == 2, let pid = pid_t(parts[0]), let start = UInt64(parts[1]) else {
      refuse("alive takes <pid>:<start>")
    }
    if let bsd = info(pid), started(bsd) == start { count += 1 }
  }
  print("alive=\(count)")
}

let args = Array(CommandLine.arguments.dropFirst())
switch args.first {
case "window":
  guard args.count == 4, let pid = pid_t(args[1]), let width = Int(args[3]) else {
    refuse("window <pid> <title> <width>")
  }
  window(pid, args[2], width)
case "tree":
  guard args.count == 2, let pid = pid_t(args[1]) else { refuse("tree <pid>") }
  tree(pid)
case "alive":
  alive(Array(args.dropFirst()))
default:
  refuse("window, tree or alive")
}
