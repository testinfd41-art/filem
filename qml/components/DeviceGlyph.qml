import QtQuick
import QtQuick.Shapes
import "DeviceGlyphPaths.js" as GlyphPaths

// A single-colour, template-style device glyph. The 24x24 SF Symbols-scale
// grid is scaled to the requested item size, so every icon shares the same
// weight and optical alignment regardless of where it appears in the sidebar.
Item {
    id: root

    property string name: "hard-drive"
    property color color: "#e7e9ee"
    property real strokeWidth: 2.0

    implicitWidth: 20
    implicitHeight: 20

    readonly property real designSize: 24
    readonly property real glyphScale: Math.min(width, height) / designSize

    Shape {
        width: root.designSize
        height: root.designSize
        anchors.centerIn: parent
        scale: root.glyphScale
        transformOrigin: Item.Center
        antialiasing: true

        ShapePath {
            fillColor: "transparent"
            strokeColor: root.color
            strokeWidth: root.strokeWidth
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: GlyphPaths.pathFor(root.name) }
        }
    }
}
