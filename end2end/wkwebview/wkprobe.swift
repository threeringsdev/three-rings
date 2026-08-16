// Minimal real-WKWebView harness: loads a URL (file:// or http://) into a
// system WKWebView, injects a driver script at document-end on every
// navigation, and waits for the page to postMessage its results through
// `window.webkit.messageHandlers.probe`. Prints them to stdout, exits.
//
//   window.webkit.messageHandlers.log.postMessage("...")     -> stderr
//   window.webkit.messageHandlers.shot.postMessage("name")   -> <shotdir>/name.png
//   window.webkit.messageHandlers.probe.postMessage(json)    -> stdout, exit 0
//
// usage: wkprobe <url> [driver.js|-] [width] [height] [timeoutSeconds] [shotdir]
import Cocoa
import WebKit

final class Probe: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
    var done = false
    var webView: WKWebView?
    var shotDir = "/tmp/wkprobe/shots"
    var pendingShots = 0
    var finalPayload: String?

    func userContentController(_ ucc: WKUserContentController, didReceive message: WKScriptMessage) {
        switch message.name {
        case "log":
            FileHandle.standardError.write("[page] \(message.body)\n".data(using: .utf8)!)
        case "shot":
            let name = "\(message.body)"
            pendingShots += 1
            let cfg = WKSnapshotConfiguration()
            cfg.afterScreenUpdates = true
            webView?.takeSnapshot(with: cfg) { image, err in
                defer {
                    self.pendingShots -= 1
                    // Let the page await the snapshot: without this the shot is
                    // dispatched async and races whatever the driver mutates next.
                    self.webView?.evaluateJavaScript(
                        "window.__shotDone && window.__shotDone(\(self.jsString(name)))", completionHandler: nil)
                    self.finishIfReady()
                }
                guard let image = image,
                      let tiff = image.tiffRepresentation,
                      let rep = NSBitmapImageRep(data: tiff),
                      let png = rep.representation(using: .png, properties: [:]) else {
                    FileHandle.standardError.write("SNAPSHOT FAIL \(name): \(String(describing: err))\n".data(using: .utf8)!)
                    return
                }
                try? FileManager.default.createDirectory(atPath: self.shotDir, withIntermediateDirectories: true)
                try? png.write(to: URL(fileURLWithPath: "\(self.shotDir)/\(name).png"))
                FileHandle.standardError.write("[shot] \(self.shotDir)/\(name).png\n".data(using: .utf8)!)
            }
        default:
            finalPayload = "\(message.body)"
            done = true
            finishIfReady()
        }
    }

    func jsString(_ s: String) -> String {
        let escaped = s.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        return "\"\(escaped)\""
    }

    func finishIfReady() {
        guard done, pendingShots == 0, let payload = finalPayload else { return }
        print(payload)
        exit(0)
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        FileHandle.standardError.write("NAV FAIL: \(error)\n".data(using: .utf8)!)
        exit(2)
    }
}

let args = CommandLine.arguments
guard args.count >= 2, let url = URL(string: args[1]) else {
    FileHandle.standardError.write("usage: wkprobe <url> [driver.js] [w] [h] [timeout] [shotdir]\n".data(using: .utf8)!)
    exit(64)
}
let driverPath = args.count > 2 ? args[2] : "-"
let w = args.count > 3 ? Double(args[3]) ?? 1280 : 1280
let h = args.count > 4 ? Double(args[4]) ?? 800 : 800
let timeout = args.count > 5 ? Double(args[5]) ?? 25 : 25

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let probe = Probe()
if args.count > 6 { probe.shotDir = args[6] }

let cfg = WKWebViewConfiguration()
cfg.userContentController.add(probe, name: "probe")
cfg.userContentController.add(probe, name: "log")
cfg.userContentController.add(probe, name: "shot")
cfg.preferences.setValue(true, forKey: "developerExtrasEnabled")

// Credentials come from the environment (end2end/.env — gitignored), never
// from a file in the repo. A driver that has to sign in reads `window.__E2E`.
let env = ProcessInfo.processInfo.environment
let e2eEmail = env["E2E_EMAIL"] ?? ""
let e2ePassword = env["E2E_PASSWORD"] ?? ""
func jsLiteral(_ s: String) -> String {
    "\"\(s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\""))\""
}
cfg.userContentController.addUserScript(
    WKUserScript(
        source: "window.__E2E = { email: \(jsLiteral(e2eEmail)), password: \(jsLiteral(e2ePassword)) };",
        injectionTime: .atDocumentStart, forMainFrameOnly: true))

if driverPath != "-", let src = try? String(contentsOfFile: driverPath, encoding: .utf8) {
    cfg.userContentController.addUserScript(
        WKUserScript(source: src, injectionTime: .atDocumentEnd, forMainFrameOnly: true)
    )
}

let rect = NSRect(x: 0, y: 0, width: w, height: h)
let webView = WKWebView(frame: rect, configuration: cfg)
webView.navigationDelegate = probe
probe.webView = webView

// A real (offscreen) window so the web view gets a genuine layout/render host.
let window = NSWindow(
    contentRect: rect,
    styleMask: [.titled, .closable],
    backing: .buffered,
    defer: false
)
window.contentView = webView
window.setFrameOrigin(NSPoint(x: -20000, y: -20000)) // offscreen: don't steal the desktop
window.orderBack(nil)

if url.isFileURL {
    webView.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
} else {
    webView.load(URLRequest(url: url))
}

DispatchQueue.main.asyncAfter(deadline: .now() + timeout) {
    if !probe.done {
        FileHandle.standardError.write("TIMEOUT after \(timeout)s\n".data(using: .utf8)!)
        exit(3)
    }
}

app.run()
