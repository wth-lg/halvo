// No window is shown, the process never becomes active, and no input event of
// the system is posted: element.click() dispatches a DOM event inside this
// offscreen view only.
import AppKit
import WebKit

guard CommandLine.arguments.count == 2 else {
    FileHandle.standardError.write("usage: xcrun swift delete-confirm.swift <path of index.html>\n".data(using: .utf8)!)
    exit(64)
}
let page = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL

let stub = """
window.__calls = [];
(function () {
  const status = (state) => ({ state, store_bytes: 3221225472, store_cap_bytes: 26843545600,
    window: { rolling_days: 14, first_report_day: 7 }, store_backend: "" });
  const invoke = (cmd, args) => {
    window.__calls.push(args && args.capture ? cmd + ":" + args.capture : cmd);
    if (cmd === "capture_status") return Promise.resolve(status("running"));
    if (cmd === "inventory") return Promise.resolve({ rows: [] });
    if (cmd === "onboarding_state") return Promise.resolve({ permissions: [], lost_grants: [], all_granted: true, walkback: [] });
    if (cmd === "set_capture_state") return Promise.resolve(status(args.capture));
    if (cmd === "delete_all") return Promise.resolve({ segments_shredded: 3, keys_destroyed: 4 });
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
out.delete_all_calls_after_second_click_at_same_point = window.__calls.filter((n) => n === "delete_all").length;
return ["at_point_before_first_click", "at_point_after_first_click", "delete_all_calls_after_second_click_at_same_point", "confirm_below_delete"]
  .map((k) => k + "=" + out[k]).join("\\n");
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
