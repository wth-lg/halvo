// No window is shown, the process never becomes active, and no input event of
// the system is posted: element.click() and dispatchEvent send DOM events
// inside this offscreen view only.
import AppKit
import WebKit

guard CommandLine.arguments.count == 3 else {
    FileHandle.standardError.write("usage: xcrun swift delete-confirm.swift <path of index.html> <directory of the capture fixtures>\n".data(using: .utf8)!)
    exit(64)
}
let page = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
let fixtures = URL(fileURLWithPath: CommandLine.arguments[2])
func fixture(_ name: String) -> String {
    guard let text = try? String(contentsOf: fixtures.appendingPathComponent(name + ".json"), encoding: .utf8) else {
        FileHandle.standardError.write("no fixture \(name)\n".data(using: .utf8)!)
        exit(2)
    }
    return text
}

let stub = """
window.__calls = [];
(function () {
  let now = 0;
  performance.now = () => now;
  window.__advance = (ms) => { now += ms; };
  window.__polls = [];
  window.setInterval = (fn, ms) => { window.__polls.push({ fn, ms }); return window.__polls.length; };
  const f = {
    status: \(fixture("capture_status-running")),
    inv_two: \(fixture("inventory-two-rows-one-day")),
    inv_empty: \(fixture("inventory-empty")),
    ob_missing: \(fixture("onboarding_state-missing")),
    ob_granted: \(fixture("onboarding_state-granted")),
    ob_none: \(fixture("onboarding_state-none")),
    deleted: \(fixture("delete_all-deleted")),
  };
  const kinds = ["screen_frame", "keystroke", "pointer", "foreground_app", "window_title"];
  const days = (n) => {
    const rows = [];
    for (let d = 0; d < n; d++) for (const modality of kinds) rows.push({ day: "2026-11-0" + (d + 1), modality, events: 10 + d, bytes: 1000 + d });
    return { rows };
  };
  f.inv_2days = days(2);
  f.inv_1day = days(1);
  window.__fixtures = f;
  window.__state = { inventory: "inv_empty", onboarding: "ob_none" };
  const invoke = (cmd, args) => {
    window.__calls.push(args && args.capture ? cmd + ":" + args.capture : cmd);
    if (cmd === "capture_status") return Promise.resolve(f.status);
    if (cmd === "inventory") return Promise.resolve(f[window.__state.inventory]);
    if (cmd === "onboarding_state") return Promise.resolve(f[window.__state.onboarding]);
    if (cmd === "set_capture_state") return Promise.resolve(Object.assign({}, f.status, { state: args.capture }));
    if (cmd === "delete_all") return Promise.resolve(f.deleted);
    if (cmd === "plugin:app|version") return Promise.resolve("0.0.0");
    return Promise.reject("no reply for " + cmd);
  };
  window.__TAURI_INTERNALS__ = { invoke };
  window.__TAURI__ = { core: { invoke } };
})();
"""

let probe = """
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
await wait(300);
const out = {};
const del = document.getElementById("delete");
del.scrollIntoView({ block: "center" });
const r = del.getBoundingClientRect();
const x = r.left + r.width / 2, y = r.top + r.height / 2;
out.at_point_before_first_click = document.elementFromPoint(x, y).id;
del.click();
const confirm = document.getElementById("confirm-delete").getBoundingClientRect();
out.confirm_below_delete = confirm.top >= r.bottom;
out.at_point_after_first_click = document.elementFromPoint(x, y).id;
document.elementFromPoint(x, y).click();
await wait(100);
const deletes = () => window.__calls.filter((n) => n === "delete_all").length;
const firstCase = deletes();
const lines = [];
const cancel = () => {
  const row = document.getElementById("delete-confirm-row");
  if (row && !row.classList.contains("hidden")) document.getElementById("cancel-delete").click();
};
const idAt = (p) => { const e = document.elementFromPoint(p[0], p[1]); return e ? (e.id || e.tagName) : "null"; };
const pressAt = (p) => {
  const el = document.elementFromPoint(p[0], p[1]);
  if (!el) return "null";
  const init = { bubbles: true, cancelable: true, view: window, detail: 1, clientX: p[0], clientY: p[1] };
  el.dispatchEvent(new PointerEvent("pointerdown", init));
  el.dispatchEvent(new MouseEvent("mousedown", init));
  el.dispatchEvent(new PointerEvent("pointerup", init));
  el.dispatchEvent(new MouseEvent("mouseup", init));
  el.dispatchEvent(new MouseEvent("click", init));
  return el.id || el.tagName;
};
const all = {};
let redrawCases = 0;
let redrawDeletes = 0;
if (window.__polls.length > 0) {
  const poll = (ms) => Promise.all(window.__polls.filter((t) => t.ms === ms).map((t) => t.fn()));
  const refresh = (name) => poll(name === "onboarding" ? 3000 : 15000);
  const scenarios = [
    ["onboarding-missing-to-granted", { inventory: "inv_empty", onboarding: "ob_missing" }, { onboarding: "ob_granted" }, "onboarding"],
    ["inventory-a-day-leaves", { inventory: "inv_2days", onboarding: "ob_none" }, { inventory: "inv_1day" }, "inventory"],
    ["inventory-two-rows-to-none", { inventory: "inv_two", onboarding: "ob_none" }, { inventory: "inv_empty" }, "inventory"],
  ];
  for (const [name, initial, change, feed] of scenarios) {
    for (const gap of [0, 10, 20, 30]) {
      const counts = {};
      let caseDeletes = 0;
      for (let i = 0; i < 15; i++) {
        cancel();
        window.__advance(20000);
        Object.assign(window.__state, initial);
        await refresh("onboarding");
        await refresh("inventory");
        const end = document.documentElement.scrollHeight - innerHeight;
        scrollTo(0, Math.max(0, end - gap));
        const a = del.getBoundingClientRect();
        const p = [a.left + 3 + ((i % 5) * (a.width - 6)) / 4, a.top + 3 + (Math.floor(i / 5) * (a.height - 6)) / 2];
        pressAt(p);
        Object.assign(window.__state, change);
        await refresh(feed);
        const y = scrollY;
        scrollTo(0, 0);
        scrollTo(0, y);
        window.__advance(700);
        const before = deletes();
        const id = pressAt(p);
        counts[id] = (counts[id] || 0) + 1;
        all[id] = (all[id] || 0) + 1;
        await Promise.resolve();
        caseDeletes += deletes() - before;
      }
      cancel();
      redrawCases += 1;
      redrawDeletes += caseDeletes;
      lines.push("redraw_case=" + name + " gap=" + gap + " at_points=" + JSON.stringify(counts) + " delete_all_calls=" + caseDeletes);
    }
  }
}
out.redraw_cases = redrawCases;
out.at_points_after_redraw = JSON.stringify(all);
out.delete_all_calls_after_second_click_at_same_point = firstCase + redrawDeletes;
return lines.concat(["at_point_before_first_click", "at_point_after_first_click", "confirm_below_delete", "redraw_cases", "at_points_after_redraw", "delete_all_calls_after_second_click_at_same_point"]
  .map((k) => k + "=" + out[k])).join("\\n");
"""

final class Loader: NSObject, WKNavigationDelegate {
    var done = false
    var result: String? = nil
    var failure = ""
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        webView.callAsyncJavaScript(probe, arguments: [:], in: nil, in: .page) { outcome in
            switch outcome {
            case .success(let value): self.result = value as? String
            case .failure(let error): self.failure = "probe failed: \(error)"
            }
            self.done = true
        }
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        failure = "load failed: \(error)"
        done = true
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        failure = "load failed: \(error)"
        done = true
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)

let config = WKWebViewConfiguration()
config.websiteDataStore = WKWebsiteDataStore.nonPersistent()
config.userContentController.addUserScript(
    WKUserScript(source: stub, injectionTime: .atDocumentStart, forMainFrameOnly: true))
let webView = WKWebView(frame: NSRect(x: 0, y: 0, width: 620, height: 720), configuration: config)
let loader = Loader()
webView.navigationDelegate = loader
webView.loadFileURL(page, allowingReadAccessTo: page.deletingLastPathComponent())

let deadline = Date().addingTimeInterval(30)
while !loader.done && Date() < deadline {
    RunLoop.main.run(until: Date().addingTimeInterval(0.05))
}
print("engine=WKWebView offscreen 620x720")
guard let result = loader.result else {
    print(loader.done ? loader.failure : "timed out after 30 s")
    exit(2)
}
print(result)
let lines = Set(result.split(separator: "\n").map(String.init))
let safe = lines.contains("at_point_after_first_click=delete")
    && lines.contains("delete_all_calls_after_second_click_at_same_point=0")
exit(safe ? 0 : 1)
