import QtQuick

Rectangle {
    id: root

    width: 340
    height: 104
    visible: backend ? backend.op_active : false
    radius: theme ? Math.max(6, theme.radius) : 8
    color: theme ? theme.surface : "#151820"
    border.width: 1
    border.color: theme ? theme.border : "#292e3a"

    property var theme
    property var backend

    Column {
        anchors.fill: parent
        anchors.margins: 14
        spacing: 8

        Text {
            width: parent.width
            text: root.backend ? root.backend.op_title : ""
            elide: Text.ElideRight
            color: theme ? theme.foreground : "#e7e9ee"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 12 * (theme ? theme.font_scale : 1)
            font.weight: Font.DemiBold
        }

        Text {
            width: parent.width
            text: root.backend ? root.backend.op_detail : ""
            elide: Text.ElideMiddle
            color: theme ? theme.muted : "#9299a8"
            font.family: theme ? theme.mono_font : "monospace"
            font.pixelSize: 10 * (theme ? theme.font_scale : 1)
        }

        Rectangle {
            width: parent.width
            height: 4
            radius: 2
            color: theme ? theme.border : "#292e3a"

            Rectangle {
                width: parent.width * Math.max(0, Math.min(1, root.backend ? root.backend.op_progress : 0))
                height: parent.height
                radius: 2
                color: theme ? theme.accent : "#8ab4f8"
            }
        }

        Row {
            width: parent.width
            height: 24

            Text {
                width: parent.width - cancelItem.width
                anchors.verticalCenter: parent.verticalCenter
                text: Math.round((root.backend ? root.backend.op_progress : 0) * 100) + "%"
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.mono_font : "monospace"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
            }

            Item {
                id: cancelItem
                width: 64
                height: parent.height
                visible: root.backend ? root.backend.op_cancelable : false

                Rectangle {
                    anchors.fill: parent
                    radius: theme ? Math.max(3, theme.radius * 0.45) : 5
                    color: cancelMouse.containsMouse
                           ? (theme ? theme.hover : "#202531")
                           : "transparent"
                    border.width: 1
                    border.color: theme ? theme.border : "#292e3a"

                    Text {
                        anchors.centerIn: parent
                        text: "Cancel"
                        color: theme ? theme.foreground : "#e7e9ee"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                    }

                    MouseArea {
                        id: cancelMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            if (root.backend)
                                root.backend.cancel_operation()
                        }
                    }
                }
            }
        }
    }
}
