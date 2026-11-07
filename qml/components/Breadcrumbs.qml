import QtQuick

Item {
    id: root

    property var theme
    property var backend
    property string dropHoverPath: ""

    signal editRequested()

    implicitHeight: 32

    Timer {
        id: dropHoverTimer
        interval: root.backend ? Math.max(200, Math.min(5000, root.backend.drag_hover_delay_ms)) : 700
        repeat: false
        onTriggered: {
            var path = root.dropHoverPath
            root.dropHoverPath = ""
            if (root.backend && path !== "" && !path.startsWith("/.__filem_")
                    && path !== root.backend.current_path)
                root.backend.navigate(path)
        }
    }

    function acceptDrop(drop, dest) {
        if (!root.backend || !dest || String(dest).startsWith("/.__filem_")
                || !drop || !drop.urls || !drop.urls.length)
            return false
        var urls = []
        for (var i = 0; i < drop.urls.length; i++)
            urls.push(drop.urls[i].toString())
        root.backend.drop_external_paths(dest, urls, drop.proposedAction === Qt.MoveAction)
        drop.acceptProposedAction()
        return true
    }

    function scrollToEnd() {
        flick.contentX = Math.max(0, flick.contentWidth - flick.width)
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: row.width
        contentHeight: height
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentWidth > width
        flickableDirection: Flickable.HorizontalFlick

        Row {
            id: row
            height: flick.height
            spacing: 0
            onWidthChanged: root.scrollToEnd()

            Repeater {
                model: root.backend ? root.backend.breadcrumbs : []

                delegate: Row {
                    height: row.height
                    spacing: 0
                    required property var modelData
                    required property int index

                    Item {
                        width: index > 0 ? 18 : 0
                        height: row.height

                        Text {
                            anchors.centerIn: parent
                            visible: index > 0
                            text: "\u203a"
                            color: theme ? theme.muted : "#9299a8"
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: 15 * (theme ? theme.font_scale : 1)
                        }
                    }

                    Rectangle {
                        height: 26
                        width: labelText.implicitWidth + 16
                        anchors.verticalCenter: parent.verticalCenter
                        radius: theme ? Math.max(3, theme.radius * 0.55) : 6
                        property bool dropTargetActive: root.dropHoverPath === modelData.path
                        color: dropTargetActive
                               ? (theme ? theme.selection : "#24364f")
                               : (crumbMouse.containsMouse
                                  ? (theme ? theme.hover : "#202531")
                                  : "transparent")
                        border.width: dropTargetActive ? 1 : 0
                        border.color: theme ? theme.selection_text : "#f2f5fb"

                        Behavior on color {
                            ColorAnimation { duration: 120 }
                        }

                        Text {
                            id: labelText
                            anchors.centerIn: parent
                            text: modelData.label
                            color: parent.dropTargetActive
                                   ? (theme ? theme.selection_text : "#f2f5fb")
                                   : (theme ? theme.foreground : "#e7e9ee")
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: 13 * (theme ? theme.font_scale : 1)

                            Behavior on color {
                                ColorAnimation { duration: 120 }
                            }
                        }

                        MouseArea {
                            id: crumbMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                if (index === root.backend.breadcrumbs.length - 1)
                                    root.editRequested()
                                else if (root.backend)
                                    root.backend.navigate(modelData.path)
                            }
                        }

                        DropArea {
                            anchors.fill: parent
                            z: 2
                            keys: ["text/uri-list", "filem/paths"]
                            onEntered: {
                                if (!String(modelData.path).startsWith("/.__filem_")) {
                                    root.dropHoverPath = modelData.path
                                    dropHoverTimer.restart()
                                }
                            }
                            onExited: {
                                if (root.dropHoverPath === modelData.path) {
                                    dropHoverTimer.stop()
                                    root.dropHoverPath = ""
                                }
                            }
                            onDropped: function(drop) {
                                dropHoverTimer.stop()
                                root.dropHoverPath = ""
                                root.acceptDrop(drop, modelData.path)
                            }
                        }
                    }
                }
            }
        }
    }

    onWidthChanged: scrollToEnd()
    Component.onCompleted: scrollToEnd()
}
