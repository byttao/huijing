import AppKit
// 用法：swift alpha.swift 文件... —— 逐个文件、逐个尺寸读四角与中心像素的 alpha
var bad = 0
for path in CommandLine.arguments.dropFirst() {
  let data = try! Data(contentsOf: URL(fileURLWithPath: path))
  var reps: [NSBitmapImageRep] = NSBitmapImageRep.imageReps(with: data).compactMap { $0 as? NSBitmapImageRep }
  if reps.isEmpty, let img = NSImage(data: data) {
    reps = img.representations.compactMap { r in
      if let b = r as? NSBitmapImageRep { return b }
      return nil
    }
  }
  for rep in reps {
    let w = rep.pixelsWide, h = rep.pixelsHigh
    func a(_ x: Int, _ y: Int) -> Int { Int((rep.colorAt(x: x, y: y)?.alphaComponent ?? -1) * 255) }
    let corners = [a(0,0), a(w-1,0), a(0,h-1), a(w-1,h-1)]
    let center = a(w/2, h/2)
    let ok = rep.hasAlpha && corners.allSatisfy { $0 == 0 } && center == 255
    if !ok { bad += 1 }
    print("\(ok ? "OK " : "BAD") \((path as NSString).lastPathComponent) \(w)x\(h) alpha通道=\(rep.hasAlpha) 四角=\(corners) 中心=\(center)")
  }
}
exit(bad == 0 ? 0 : 1)
