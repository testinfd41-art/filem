import QtQuick

Item {
    id: root

    anchors.fill: parent
    z: 450
    visible: open

    property var theme
    property var tags: []
    property bool open: false

    signal chosen(int index)
    signal canceled()

    function close() {
        if (!open)
            return
        open = false
        canceled()
    }

    Rectangle {
        anchors.fill: parent
        color: "#000000"
        opacity: 0.5
        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }
    }

    Rectangle {
        width: Math.min(300, root.width - 48)
        height: content.height + 40
        anchors.centerIn: parent
        radius: root.theme ? Math.max(6, root.theme.radius) : 10
        color: root.theme ? root.theme.surface : "#151820"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        Column {
            id: content
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: 20
            spacing: 6

            Text {
                text: "Toggle Tag"
                color: root.theme ? root.theme.foreground : "#e7e9ee"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 15 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
                bottomPadding: 6
            }

            Repeater {
                model: root.tags
                delegate: Rectangle {
                    required property var modelData
                    width: content.width
                    height: 34
                    radius: 5
                    color: tagMouse.containsMouse
                           ? (root.theme ? root.theme.hover : "#202531")
                           : "transparent"

                    Rectangle {
                        anchors.left: parent.left
                        anchors.leftMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        width: 10
                        height: 10
                        radius: 5
                        color: modelData.color || (root.theme ? root.theme.tag_color(modelData.index) : "#9299a8")
                    }

                    Text {
                        anchors.fill: parent
                        anchors.leftMargin: 26
                        verticalAlignment: Text.AlignVCenter
                        text: modelData.name
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    }

                    MouseArea {
                        id: tagMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            var index = modelData.index
                            root.open = false
                            root.chosen(index)
                        }
                    }
                }
            }

            Rectangle {
                width: content.width
                height: 34
                radius: 5
                color: clearMouse.containsMouse
                       ? (root.theme ? root.theme.hover : "#202531")
                       : "transparent"
                Text {
                    anchors.fill: parent
                    anchors.leftMargin: 26
                    verticalAlignment: Text.AlignVCenter
                    text: "Remove All Tags"
                    color: root.theme ? root.theme.muted : "#9299a8"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }
                MouseArea {
                    id: clearMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        root.open = false
                        root.chosen(-1)
                    }
                }
            }
        }
    }

    Shortcut {
        sequence: "Escape"
        context: Qt.WindowShortcut
        enabled: root.open
        onActivated: root.close()
    }
}
