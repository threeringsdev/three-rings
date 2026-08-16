import CoreGraphics
import Foundation
let pid = Int(CommandLine.arguments[1])!
guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { exit(1) }
for w in list {
    guard let owner = w[kCGWindowOwnerPID as String] as? Int, owner == pid else { continue }
    let num = w[kCGWindowNumber as String] as? Int ?? -1
    let bounds = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let name = w[kCGWindowName as String] as? String ?? ""
    let layer = w[kCGWindowLayer as String] as? Int ?? -1
    print("\(num)\t layer=\(layer)\t \(bounds)\t \"\(name)\"")
}
