// Renders the Rayperclip app icon: the 🖇️ emoji on a blue gradient squircle.
//
// Usage: swift scripts/render-icon.swift <output.png>
//
// Produces a 1024×1024 PNG following Apple's macOS icon grid
// (824 px rounded square with a 100 px margin).

import AppKit

// MARK: - Tweakable constants

let canvas: CGFloat = 1024
let tileSize: CGFloat = 824
let cornerRadius: CGFloat = 185
let emoji = "🖇️"
let emojiScale: CGFloat = 0.60  // emoji size relative to the tile

/// Gradient from top to bottom.
let gradientColors: [NSColor] = [
    NSColor(srgbRed: 0x5A / 255, green: 0xC8 / 255, blue: 0xFA / 255, alpha: 1),  // sky blue
    NSColor(srgbRed: 0x0A / 255, green: 0x84 / 255, blue: 0xFF / 255, alpha: 1),  // system blue
    NSColor(srgbRed: 0x1E / 255, green: 0x3A / 255, blue: 0x8A / 255, alpha: 1),  // deep indigo
]
let gradientLocations: [CGFloat] = [0.0, 0.5, 1.0]

// MARK: - Rendering

guard CommandLine.arguments.count == 2 else {
    FileHandle.standardError.write("usage: render-icon.swift <output.png>\n".data(using: .utf8)!)
    exit(2)
}
let outputURL = URL(fileURLWithPath: CommandLine.arguments[1])

guard let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(canvas), pixelsHigh: Int(canvas),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
else { fatalError("cannot create bitmap") }
rep.size = NSSize(width: canvas, height: canvas)

NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSGraphicsContext.current?.imageInterpolation = .high

let margin = (canvas - tileSize) / 2
let tileRect = NSRect(x: margin, y: margin, width: tileSize, height: tileSize)
let tile = NSBezierPath(roundedRect: tileRect, xRadius: cornerRadius, yRadius: cornerRadius)

// Soft drop shadow under the tile.
NSGraphicsContext.saveGraphicsState()
let tileShadow = NSShadow()
tileShadow.shadowColor = NSColor.black.withAlphaComponent(0.30)
tileShadow.shadowOffset = NSSize(width: 0, height: -12)
tileShadow.shadowBlurRadius = 28
tileShadow.set()
NSColor(srgbRed: 0x0A / 255, green: 0x84 / 255, blue: 0xFF / 255, alpha: 1).setFill()
tile.fill()
NSGraphicsContext.restoreGraphicsState()

// Gradient fill (angle -90 = top to bottom).
NSGradient(colors: gradientColors, atLocations: gradientLocations, colorSpace: .sRGB)!
    .draw(in: tile, angle: -90)

// Glossy highlight on the upper part of the tile.
NSGraphicsContext.saveGraphicsState()
tile.addClip()
let glossRect = NSRect(
    x: margin - tileSize * 0.25, y: margin + tileSize * 0.45,
    width: tileSize * 1.5, height: tileSize * 0.9)
NSGradient(colors: [NSColor.white.withAlphaComponent(0.28), NSColor.white.withAlphaComponent(0.0)])!
    .draw(in: NSBezierPath(ovalIn: glossRect), angle: -90)
NSGraphicsContext.restoreGraphicsState()

// Thin inner stroke for definition.
NSColor.white.withAlphaComponent(0.18).setStroke()
let stroke = NSBezierPath(
    roundedRect: tileRect.insetBy(dx: 2, dy: 2), xRadius: cornerRadius - 2, yRadius: cornerRadius - 2)
stroke.lineWidth = 4
stroke.stroke()

// The emoji, centered, with a soft shadow.
let fontSize = tileSize * emojiScale
let font = NSFont(name: "Apple Color Emoji", size: fontSize) ?? NSFont.systemFont(ofSize: fontSize)
let emojiShadow = NSShadow()
emojiShadow.shadowColor = NSColor.black.withAlphaComponent(0.35)
emojiShadow.shadowOffset = NSSize(width: 0, height: -10)
emojiShadow.shadowBlurRadius = 24
let text = NSAttributedString(string: emoji, attributes: [.font: font, .shadow: emojiShadow])
let size = text.size()
text.draw(at: NSPoint(x: (canvas - size.width) / 2, y: (canvas - size.height) / 2))

NSGraphicsContext.restoreGraphicsState()

guard let png = rep.representation(using: .png, properties: [:]) else { fatalError("PNG encoding failed") }
do {
    try png.write(to: outputURL)
} catch {
    FileHandle.standardError.write("cannot write \(outputURL.path): \(error)\n".data(using: .utf8)!)
    exit(1)
}
