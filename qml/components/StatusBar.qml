import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root

    property var theme
    property var backend
    property var fileArea

    color: theme ? theme.surface : "#151820"

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: 1
        color: theme ? theme.border : "#292e3a"
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 12

        Text {
            text: root.backend ? root.backend.status : ""
            color: theme ? theme.foreground : "#e7e9ee"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
            elide: Text.ElideRight
        }

        Text {
            visible: text !== ""
            text: root.fileArea ? root.fileArea.selectionText : ""
            color: theme ? theme.accent : "#8ab4f8"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
            elide: Text.ElideRight
        }

        Item { Layout.fillWidth: true }

        Text {
            text: root.backend
                  ? root.backend.sort_key + (root.backend.sort_ascending ? " \u2191" : " \u2193")
                    + (root.backend.directories_first ? " \u00b7 folders first" : "")
                  : ""
            color: theme ? theme.muted : "#9299a8"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 10 * (theme ? theme.font_scale : 1)
        }

        Text {
            visible: root.backend && root.backend.show_hidden
            text: "hidden shown"
            color: theme ? theme.muted : "#9299a8"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 10 * (theme ? theme.font_scale : 1)
        }

        Text {
            text: root.theme ? root.theme.source : ""
            color: theme ? theme.muted : "#9299a8"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 10 * (theme ? theme.font_scale : 1)
            opacity: 0.85
        }
    }
}
