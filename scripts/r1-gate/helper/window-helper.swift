import AppKit

let seconds = CommandLine.arguments.count == 2 ? Double(CommandLine.arguments[1]) ?? 0 : 0
guard seconds > 0 else {
    FileHandle.standardError.write(Data("usage: window-helper SECONDS\n".utf8))
    exit(64)
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

guard let screen = NSScreen.main else {
    FileHandle.standardError.write(Data("window-helper: no screen\n".utf8))
    exit(1)
}

let side: CGFloat = 48
let visible = screen.visibleFrame
let rect = NSRect(x: visible.maxX - side - 8, y: visible.maxY - side - 8, width: side, height: side)
let window = NSWindow(contentRect: rect, styleMask: [.borderless], backing: .buffered, defer: false)
window.backgroundColor = NSColor(srgbRed: 1, green: 0, blue: 1, alpha: 1)
window.isOpaque = true
window.hasShadow = false
window.ignoresMouseEvents = true
window.level = .floating
window.isReleasedWhenClosed = false

var step = 0
let repaint = Timer(timeInterval: 1.0 / 60.0, repeats: true) { _ in
    step = (step + 1) % 120
    let ramp = CGFloat(step < 60 ? step : 120 - step) / 60.0
    window.backgroundColor = NSColor(srgbRed: 1, green: ramp * 0.5, blue: 1 - ramp * 0.5, alpha: 1)
}
RunLoop.main.add(repaint, forMode: .common)

window.orderFrontRegardless()
print("helper visible pid=\(getpid())")
fflush(stdout)

DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
    exit(0)
}
app.run()
