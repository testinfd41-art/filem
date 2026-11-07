import QtQuick

Item {
    id: root

    property string name: ""
    property color color: "#e7e9ee"
    property bool filled: false
    implicitWidth: 18
    implicitHeight: 18

    Repeater {
        model: root.name === "view-grid" ? 4 : 0

        Rectangle {
            required property int index
            width: 6
            height: 6
            radius: 1
            color: "transparent"
            border.width: 1.5
            border.color: root.color
            x: index % 2 === 0 ? 2 : 10
            y: index < 2 ? 2 : 10
        }
    }

    Repeater {
        model: root.name === "view-list" ? 3 : 0

        Item {
            required property int index
            x: 2
            y: 3 + index * 6
            width: 14
            height: 4

            Rectangle {
                width: 3
                height: 3
                radius: 1
                color: root.color
            }
            Rectangle {
                x: 5
                y: 1
                width: 9
                height: 1.5
                radius: 1
                color: root.color
            }
        }
    }

    Repeater {
        model: root.name === "view-columns" ? 3 : 0

        Rectangle {
            required property int index
            x: 2 + index * 5
            y: 2
            width: 4
            height: 14
            radius: 1
            color: "transparent"
            border.width: 1.4
            border.color: root.color
        }
    }

    Rectangle {
        visible: root.name === "search"
        x: 2
        y: 2
        width: 10
        height: 10
        radius: 5
        color: "transparent"
        border.width: 1.7
        border.color: root.color

        Rectangle {
            x: 8
            y: 8
            width: 6
            height: 1.7
            radius: 1
            color: root.color
            rotation: 45
            transformOrigin: Item.Left
        }
    }

    Rectangle {
        visible: root.name === "preview" || root.name === "details"
        x: 1.5
        y: 2
        width: 15
        height: 14
        radius: 2
        color: "transparent"
        border.width: 1.5
        border.color: root.color

        Rectangle {
            x: 9
            y: 1
            width: 1.3
            height: 11
            color: root.color
        }
        Rectangle {
            x: 11
            y: 4
            width: 3
            height: 1.2
            radius: 1
            color: root.color
        }
        Rectangle {
            x: 11
            y: 7
            width: 3
            height: 1.2
            radius: 1
            color: root.color
        }
    }

    Canvas {
        id: customGlyph
        visible: [
            "bookmark", "refresh", "location", "home", "desktop", "documents",
            "downloads", "music", "pictures", "videos", "trash", "recent",
            "folder", "drive", "removable-drive", "system-drive", "network",
            "phone", "camera", "optical-drive", "encrypted-drive", "plus"
        ].indexOf(root.name) >= 0
        anchors.fill: parent
        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)
            ctx.strokeStyle = root.color
            ctx.fillStyle = root.color
            ctx.lineWidth = 1.7
            ctx.lineJoin = "round"
            ctx.lineCap = "round"
            if (root.name === "bookmark") {
                ctx.beginPath()
                ctx.moveTo(5, 2.5)
                ctx.lineTo(13, 2.5)
                ctx.lineTo(13, 15.5)
                ctx.lineTo(9, 12.5)
                ctx.lineTo(5, 15.5)
                ctx.closePath()
                if (root.filled)
                    ctx.fill()
                else
                    ctx.stroke()
            } else if (root.name === "refresh") {
                ctx.beginPath()
                ctx.arc(9, 9, 6, -Math.PI * 0.72, Math.PI * 0.78)
                ctx.stroke()
                ctx.beginPath()
                ctx.moveTo(2.5, 4)
                ctx.lineTo(2.5, 8)
                ctx.lineTo(6.5, 8)
                ctx.stroke()
            } else if (root.name === "location") {
                ctx.beginPath()
                ctx.moveTo(9, 16)
                ctx.bezierCurveTo(7.5, 13, 3.5, 9.5, 3.5, 7)
                ctx.arc(9, 7, 5.5, Math.PI, 0, true)
                ctx.bezierCurveTo(14.5, 9.5, 10.5, 13, 9, 16)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(9, 7, 1.7, 0, Math.PI * 2)
                ctx.stroke()
            } else if (root.name === "home") {
                ctx.beginPath()
                ctx.moveTo(2, 8)
                ctx.lineTo(9, 2)
                ctx.lineTo(16, 8)
                ctx.moveTo(4.5, 7)
                ctx.lineTo(4.5, 16)
                ctx.lineTo(13.5, 16)
                ctx.lineTo(13.5, 7)
                ctx.moveTo(8, 16)
                ctx.lineTo(8, 11)
                ctx.lineTo(10, 11)
                ctx.lineTo(10, 16)
                ctx.stroke()
            } else if (root.name === "desktop") {
                ctx.strokeRect(2, 3, 14, 10)
                ctx.beginPath()
                ctx.moveTo(9, 13)
                ctx.lineTo(9, 16)
                ctx.moveTo(6, 16)
                ctx.lineTo(12, 16)
                ctx.stroke()
            } else if (root.name === "pictures") {
                ctx.strokeRect(2.5, 3, 13, 12)
                ctx.beginPath()
                ctx.arc(6.5, 7, 1, 0, Math.PI * 2)
                ctx.moveTo(3.5, 13)
                ctx.lineTo(7.5, 9.5)
                ctx.lineTo(10, 12)
                ctx.lineTo(12, 10)
                ctx.lineTo(15, 13)
                ctx.stroke()
            } else if (root.name === "music") {
                ctx.beginPath()
                ctx.moveTo(10, 12)
                ctx.lineTo(10, 3)
                ctx.lineTo(15, 2)
                ctx.lineTo(15, 11)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(7.5, 13, 1.8, 0, Math.PI * 2)
                ctx.arc(12.5, 12, 1.8, 0, Math.PI * 2)
                ctx.stroke()
            } else if (root.name === "videos") {
                ctx.beginPath()
                ctx.moveTo(2.5, 4)
                ctx.lineTo(15.5, 4)
                ctx.lineTo(15.5, 14)
                ctx.lineTo(2.5, 14)
                ctx.closePath()
                ctx.moveTo(7, 7)
                ctx.lineTo(7, 11)
                ctx.lineTo(10.5, 9)
                ctx.closePath()
                ctx.stroke()
            } else if (root.name === "documents") {
                ctx.beginPath()
                ctx.moveTo(4, 2)
                ctx.lineTo(10.5, 2)
                ctx.lineTo(14, 5.5)
                ctx.lineTo(14, 16)
                ctx.lineTo(4, 16)
                ctx.closePath()
                ctx.moveTo(10, 2.5)
                ctx.lineTo(10, 6)
                ctx.lineTo(13.5, 6)
                ctx.moveTo(6.5, 9)
                ctx.lineTo(11.5, 9)
                ctx.moveTo(6.5, 12)
                ctx.lineTo(11.5, 12)
                ctx.stroke()
            } else if (root.name === "downloads") {
                ctx.beginPath()
                ctx.moveTo(9, 2)
                ctx.lineTo(9, 11)
                ctx.moveTo(5.5, 7.5)
                ctx.lineTo(9, 11)
                ctx.lineTo(12.5, 7.5)
                ctx.moveTo(3, 13)
                ctx.lineTo(3, 16)
                ctx.lineTo(15, 16)
                ctx.lineTo(15, 13)
                ctx.stroke()
            } else if (root.name === "trash") {
                ctx.beginPath()
                ctx.moveTo(4, 5)
                ctx.lineTo(14, 5)
                ctx.moveTo(7, 3)
                ctx.lineTo(11, 3)
                ctx.moveTo(5, 6)
                ctx.lineTo(6, 16)
                ctx.lineTo(12, 16)
                ctx.lineTo(13, 6)
                ctx.moveTo(8, 8)
                ctx.lineTo(8, 14)
                ctx.moveTo(10, 8)
                ctx.lineTo(10, 14)
                ctx.stroke()
            } else if (root.name === "recent") {
                ctx.beginPath()
                ctx.arc(9, 9, 6.5, 0, Math.PI * 2)
                ctx.moveTo(9, 5)
                ctx.lineTo(9, 9)
                ctx.lineTo(12, 10.5)
                ctx.stroke()
            } else if (root.name === "system-drive") {
                // macOS-style root system drive: horizontal sled, tray seam, LED, slot, and processor accent.
                ctx.beginPath()
                ctx.moveTo(4.5, 4)
                ctx.lineTo(13.5, 4)
                ctx.quadraticCurveTo(15.5, 4, 15.5, 6)
                ctx.lineTo(15.5, 12)
                ctx.quadraticCurveTo(15.5, 14, 13.5, 14)
                ctx.lineTo(4.5, 14)
                ctx.quadraticCurveTo(2.5, 14, 2.5, 12)
                ctx.lineTo(2.5, 6)
                ctx.quadraticCurveTo(2.5, 4, 4.5, 4)
                ctx.stroke()
                ctx.beginPath()
                ctx.moveTo(2.5, 10.5)
                ctx.lineTo(15.5, 10.5)
                ctx.moveTo(4.8, 12.2)
                ctx.lineTo(8.5, 12.2)
                ctx.moveTo(7, 7.2)
                ctx.lineTo(11, 7.2)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(13.2, 12.2, 0.75, 0, Math.PI * 2)
                ctx.stroke()
            } else if (root.name === "drive") {
                // macOS-style internal disk: horizontal sled, tray seam, LED dot, and slot.
                ctx.beginPath()
                ctx.moveTo(4.5, 4)
                ctx.lineTo(13.5, 4)
                ctx.quadraticCurveTo(15.5, 4, 15.5, 6)
                ctx.lineTo(15.5, 12)
                ctx.quadraticCurveTo(15.5, 14, 13.5, 14)
                ctx.lineTo(4.5, 14)
                ctx.quadraticCurveTo(2.5, 14, 2.5, 12)
                ctx.lineTo(2.5, 6)
                ctx.quadraticCurveTo(2.5, 4, 4.5, 4)
                ctx.stroke()
                ctx.beginPath()
                ctx.moveTo(2.5, 10.5)
                ctx.lineTo(15.5, 10.5)
                ctx.moveTo(4.8, 12.2)
                ctx.lineTo(8.5, 12.2)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(13.2, 12.2, 0.75, 0, Math.PI * 2)
                ctx.stroke()
            } else if (root.name === "removable-drive") {
                // USB flash drive: body, connector, and contact pins.
                ctx.beginPath()
                ctx.moveTo(6, 6)
                ctx.lineTo(12, 6)
                ctx.quadraticCurveTo(13, 6, 13, 7)
                ctx.lineTo(13, 14.5)
                ctx.quadraticCurveTo(13, 15.5, 12, 15.5)
                ctx.lineTo(6, 15.5)
                ctx.quadraticCurveTo(5, 15.5, 5, 14.5)
                ctx.lineTo(5, 7)
                ctx.quadraticCurveTo(5, 6, 6, 6)
                ctx.stroke()
                ctx.beginPath()
                ctx.moveTo(7.5, 6)
                ctx.lineTo(7.5, 2.5)
                ctx.lineTo(10.5, 2.5)
                ctx.lineTo(10.5, 6)
                ctx.moveTo(8.3, 4)
                ctx.lineTo(8.3, 4.8)
                ctx.moveTo(9.7, 4)
                ctx.lineTo(9.7, 4.8)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(9, 11, 1.3, 0, Math.PI * 2)
                ctx.stroke()
            } else if (root.name === "phone") {
                ctx.beginPath()
                ctx.moveTo(5.5, 2.5)
                ctx.lineTo(12.5, 2.5)
                ctx.quadraticCurveTo(13.5, 2.5, 13.5, 3.5)
                ctx.lineTo(13.5, 14.5)
                ctx.quadraticCurveTo(13.5, 15.5, 12.5, 15.5)
                ctx.lineTo(5.5, 15.5)
                ctx.quadraticCurveTo(4.5, 15.5, 4.5, 14.5)
                ctx.lineTo(4.5, 3.5)
                ctx.quadraticCurveTo(4.5, 2.5, 5.5, 2.5)
                ctx.stroke()
                ctx.beginPath()
                ctx.moveTo(7.5, 4.2)
                ctx.lineTo(10.5, 4.2)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(9, 13.4, 0.8, 0, Math.PI * 2)
                ctx.stroke()
            } else if (root.name === "camera") {
                ctx.beginPath()
                ctx.moveTo(3, 5.5)
                ctx.lineTo(6.2, 5.5)
                ctx.lineTo(7.3, 3.5)
                ctx.lineTo(10.7, 3.5)
                ctx.lineTo(11.8, 5.5)
                ctx.lineTo(15, 5.5)
                ctx.quadraticCurveTo(16, 5.5, 16, 6.5)
                ctx.lineTo(16, 13.5)
                ctx.quadraticCurveTo(16, 14.5, 15, 14.5)
                ctx.lineTo(3, 14.5)
                ctx.quadraticCurveTo(2, 14.5, 2, 13.5)
                ctx.lineTo(2, 6.5)
                ctx.quadraticCurveTo(2, 5.5, 3, 5.5)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(9, 10, 2.8, 0, Math.PI * 2)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(13, 7.8, 0.6, 0, Math.PI * 2)
                ctx.stroke()
            } else if (root.name === "optical-drive") {
                ctx.beginPath()
                ctx.arc(9, 9, 6.5, 0, Math.PI * 2)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(9, 9, 1.8, 0, Math.PI * 2)
                ctx.moveTo(9, 2.5)
                ctx.lineTo(9, 5.2)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(9, 9, 4.5, -0.9, 0.2)
                ctx.stroke()
            } else if (root.name === "encrypted-drive") {
                ctx.beginPath()
                ctx.moveTo(4.5, 4)
                ctx.lineTo(13.5, 4)
                ctx.quadraticCurveTo(15.5, 4, 15.5, 6)
                ctx.lineTo(15.5, 13)
                ctx.quadraticCurveTo(15.5, 15, 13.5, 15)
                ctx.lineTo(4.5, 15)
                ctx.quadraticCurveTo(2.5, 15, 2.5, 13)
                ctx.lineTo(2.5, 6)
                ctx.quadraticCurveTo(2.5, 4, 4.5, 4)
                ctx.stroke()
                ctx.beginPath()
                ctx.rect(8.5, 9, 5, 4.2)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(11, 9, 1.8, Math.PI, 0)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(11, 11, 0.55, 0, Math.PI * 2)
                ctx.stroke()
            } else if (root.name === "network") {
                ctx.beginPath()
                ctx.arc(9, 9, 6.5, 0, Math.PI * 2)
                ctx.moveTo(2.8, 6.5)
                ctx.lineTo(15.2, 6.5)
                ctx.moveTo(2.8, 11.5)
                ctx.lineTo(15.2, 11.5)
                ctx.moveTo(9, 2.5)
                ctx.bezierCurveTo(5.5, 6, 5.5, 12, 9, 15.5)
                ctx.moveTo(9, 2.5)
                ctx.bezierCurveTo(12.5, 6, 12.5, 12, 9, 15.5)
                ctx.stroke()
            } else if (root.name === "plus") {
                ctx.beginPath()
                ctx.moveTo(9, 3)
                ctx.lineTo(9, 15)
                ctx.moveTo(3, 9)
                ctx.lineTo(15, 9)
                ctx.stroke()
            } else {
                ctx.beginPath()
                ctx.moveTo(2, 6)
                ctx.lineTo(7, 6)
                ctx.lineTo(8.5, 8)
                ctx.lineTo(16, 8)
                ctx.lineTo(16, 15)
                ctx.lineTo(2, 15)
                ctx.closePath()
                ctx.stroke()
            }
        }
        Component.onCompleted: requestPaint()
    }

    onColorChanged: customGlyph.requestPaint()
    onFilledChanged: customGlyph.requestPaint()
    onNameChanged: customGlyph.requestPaint()
}
