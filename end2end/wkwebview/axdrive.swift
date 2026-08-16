// pid-keyed accessibility driver. AXUIElementCreateApplication takes a pid, so
// unlike System Events / Apple Events it cannot alias two processes that share
// a name and CFBundleIdentifier (which two three-rings builds do).
//
//   axdrive <pid> resize <x> <y> <w> <h>
//   axdrive <pid> raise
//   axdrive <pid> dump [maxDepth]
//   axdrive <pid> find <substring>
//   axdrive <pid> press <substring>
import ApplicationServices
import Foundation

let args = CommandLine.arguments
guard args.count >= 3, let pid = pid_t(args[1]) else {
    FileHandle.standardError.write("usage: axdrive <pid> <cmd> ...\n".data(using: .utf8)!)
    exit(64)
}
let app = AXUIElementCreateApplication(pid)
// WKWebView only builds its accessibility tree once a client asks for it. In a
// plain Cocoa host (Tauri/wry) that opt-in is this attribute; without it the
// web area never appears and the app looks like an empty AXGroup.
AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, kCFBooleanTrue)
AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
usleep(400_000)

func copyAttr(_ el: AXUIElement, _ attr: String) -> CFTypeRef? {
    var v: CFTypeRef?
    return AXUIElementCopyAttributeValue(el, attr as CFString, &v) == .success ? v : nil
}
func str(_ el: AXUIElement, _ attr: String) -> String {
    guard let v = copyAttr(el, attr) else { return "" }
    if let s = v as? String { return s }
    if let n = v as? NSNumber { return n.stringValue }
    return ""
}
func children(_ el: AXUIElement) -> [AXUIElement] {
    let kids = (copyAttr(el, kAXChildrenAttribute as String) as? [AXUIElement]) ?? []
    // The app element occasionally reports itself as its own child; that walks
    // forever. Drop any child equal to the element we are expanding.
    return kids.filter { !CFEqual($0, el) }
}
func frame(_ el: AXUIElement) -> String {
    var p = CGPoint.zero, s = CGSize.zero
    if let pv = copyAttr(el, kAXPositionAttribute as String) {
        AXValueGetValue(pv as! AXValue, .cgPoint, &p)
    }
    if let sv = copyAttr(el, kAXSizeAttribute as String) {
        AXValueGetValue(sv as! AXValue, .cgSize, &s)
    }
    return "{\(Int(p.x)),\(Int(p.y))} \(Int(s.width))x\(Int(s.height))"
}
func label(_ el: AXUIElement) -> String {
    let bits = [str(el, kAXTitleAttribute as String), str(el, kAXDescriptionAttribute as String),
                str(el, kAXValueAttribute as String)].filter { !$0.isEmpty }
    return bits.joined(separator: " | ")
}

let cmd = args[2]
switch cmd {
case "resize":
    guard args.count >= 7, let win = (copyAttr(app, kAXWindowsAttribute as String) as? [AXUIElement])?.first else {
        print("no window"); exit(1)
    }
    var p = CGPoint(x: Double(args[3])!, y: Double(args[4])!)
    var s = CGSize(width: Double(args[5])!, height: Double(args[6])!)
    AXUIElementSetAttributeValue(win, kAXPositionAttribute as CFString, AXValueCreate(.cgPoint, &p)!)
    AXUIElementSetAttributeValue(win, kAXSizeAttribute as CFString, AXValueCreate(.cgSize, &s)!)
    print("window now \(frame(win))")

case "raise":
    if let win = (copyAttr(app, kAXWindowsAttribute as String) as? [AXUIElement])?.first {
        AXUIElementPerformAction(win, kAXRaiseAction as CFString)
    }
    AXUIElementSetAttributeValue(app, kAXFrontmostAttribute as CFString, kCFBooleanTrue)
    print("raised")

case "dump":
    let maxDepth = args.count > 3 ? Int(args[3])! : 12
    func walk(_ el: AXUIElement, _ depth: Int, _ path: String) {
        let role = str(el, kAXRoleAttribute as String)
        let lab = label(el)
        if depth > 0 || !role.isEmpty {
            print("\(String(repeating: "  ", count: depth))[\(path)] \(role) \(frame(el)) \(lab.prefix(90))")
        }
        guard depth < maxDepth else { return }
        for (i, c) in children(el).enumerated() { walk(c, depth + 1, "\(path).\(i)") }
    }
    walk(app, 0, "0")

case "find", "press":
    guard args.count >= 4 else { print("need substring"); exit(64) }
    let needle = args[3].lowercased()
    var hits: [(AXUIElement, String, String)] = []
    func walk(_ el: AXUIElement, _ depth: Int, _ path: String) {
        let role = str(el, kAXRoleAttribute as String)
        let lab = label(el)
        if lab.lowercased().contains(needle) || role.lowercased().contains(needle) {
            hits.append((el, path, "\(role) \(frame(el)) \(lab.prefix(80))"))
        }
        guard depth < 40 else { return }
        for (i, c) in children(el).enumerated() { walk(c, depth + 1, "\(path).\(i)") }
    }
    walk(app, 0, "0")
    if cmd == "find" {
        for (_, path, desc) in hits { print("[\(path)] \(desc)") }
    } else {
        guard let first = hits.first else { print("NO MATCH for \(needle)"); exit(1) }
        let r = AXUIElementPerformAction(first.0, kAXPressAction as CFString)
        print("pressed [\(first.1)] \(first.2) -> \(r == .success ? "ok" : "err \(r.rawValue)")")
    }

case "click":
    guard args.count >= 5, let x = Double(args[3]), let y = Double(args[4]) else { print("need x y"); exit(64) }
    let pt = CGPoint(x: x, y: y)
    let down = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: pt, mouseButton: .left)
    let up = CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: pt, mouseButton: .left)
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: pt, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(120_000)
    down?.post(tap: .cghidEventTap)
    usleep(60_000)
    up?.post(tap: .cghidEventTap)
    print("clicked \(Int(x)),\(Int(y))")

default:
    print("unknown cmd \(cmd)"); exit(64)
}
