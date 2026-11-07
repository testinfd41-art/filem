import QtQuick
import QtQuick.Controls

Button {
    id: root

    property var theme
    property bool emphasized: false
    property bool destructive: false

    implicitHeight: 32
    leftPadding: 12
    rightPadding: 12
    font.family: theme ? theme.font_family : "sans-serif"
    font.pixelSize: 12 * (theme ? theme.font_scale : 1)

    contentItem: Text {
        text: root.text
        color: !root.enabled
               ? (root.theme ? root.theme.muted : "#9299a8")
               : (root.destructive
                  ? (root.hovered
                     ? (root.theme ? root.theme.surface : "#151820")
                     : (root.theme ? root.theme.danger : "#e06c75"))
                  : (root.emphasized
                     ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                     : (root.theme ? root.theme.foreground : "#e7e9ee")))
        font: root.font
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }

    background: Rectangle {
        radius: root.theme ? Math.max(6, root.theme.radius * 0.55) : 6
        color: {
            if (!root.enabled || (root.flat && !root.hovered))
                return "transparent"
            if (root.destructive)
                return root.hovered
                       ? (root.theme ? Qt.darker(root.theme.danger, 1.35) : "#a84d46")
                                    : (root.theme ? root.theme.surface_alt : "#1c202a")
            if (root.emphasized)
                return root.hovered ? (root.theme ? Qt.darker(root.theme.selection, 1.08) : "#1f2e43")
                                    : (root.theme ? root.theme.selection : "#24364f")
            return root.down
                   ? (root.theme ? root.theme.selection : "#24364f")
                   : (root.hovered ? (root.theme ? root.theme.hover : "#202531")
                                   : (root.theme ? root.theme.surface_alt : "#1c202a"))
        }
        border.width: root.activeFocus || (!root.flat && root.enabled) ? 1 : 0
        border.color: root.activeFocus
                      ? (root.theme ? root.theme.focus : "#8ab4f8")
                      : (root.destructive
                         ? (root.theme ? root.theme.danger : "#e06c75")
                         : (root.theme ? root.theme.border : "#292e3a"))

        Behavior on color {
            ColorAnimation { duration: 110; easing.type: Easing.OutCubic }
        }
    }
}
