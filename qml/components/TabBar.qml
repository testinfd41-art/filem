import QtQuick

Rectangle {
    id: root

    height: 36
    color: theme ? theme.background : "#0f1115"

    property var theme
    property var backend
    property int menuTab: -1
    property int dropHoverTab: -1

    Timer {
        id: dropHoverTimer
        interval: root.backend ? Math.max(200, Math.min(5000, root.backend.drag_hover_delay_ms)) : 700
        repeat: false
        onTriggered: {
            var index = root.dropHoverTab
            root.dropHoverTab = -1
            var tab = root.backend && root.backend.tabs[index]
            if (tab && tab.path && !String(tab.path).startsWith("/.__filem_"))
                root.backend.select_tab(index)
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

    function indexAt(px) {
        for (var i = 0; i < tabs.count; i++) {
            var item = tabs.itemAt(i)
            if (item && px < item.x + item.width)
                return i
        }
        return tabs.count - 1
    }

    function ensureVisible(index) {
        var item = tabs.itemAt(index)
        if (!item)
            return
        if (item.x < flick.contentX)
            flick.contentX = item.x
        else if (item.x + item.width > flick.contentX + flick.width)
            flick.contentX = Math.max(0, item.x + item.width - flick.width)
    }

    function tabMenuEntries(index) {
        return [
            { label: "New Tab", shortcut: "Ctrl+T", action: function() { backend.new_tab("") } },
            { label: "Duplicate Tab", action: function() { backend.duplicate_tab(index) } },
            { label: "Close Tab", shortcut: "Ctrl+W", action: function() { backend.close_tab(index) } },
            { separator: true },
            { label: "Reopen Closed Tab", shortcut: "Ctrl+Shift+T", action: function() { backend.reopen_tab() } },
            { separator: true },
            { label: "Move Tab Left", action: function() { backend.move_tab(index, index - 1) } },
            { label: "Move Tab Right", action: function() { backend.move_tab(index, index + 1) } },
            { separator: true },
            { label: "Close Other Tabs", enabled: tabs.count > 1, action: function() {
                // Closing from the end keeps the indices of earlier tabs valid.
                for (var i = tabs.count - 1; i >= 0; i--) {
                    if (i !== index)
                        backend.close_tab(i)
                }
            } }
        ]
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.NoButton
        onWheel: function(wheel) {
            if (flick.contentWidth <= flick.width) {
                wheel.accepted = false
                return
            }
            flick.contentX = Math.max(
                0,
                Math.min(flick.contentWidth - flick.width,
                         flick.contentX - wheel.angleDelta.y - wheel.angleDelta.x)
            )
            wheel.accepted = true
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: tabsRow.width
        contentHeight: height
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: false

        Row {
            id: tabsRow
            height: flick.height

            Repeater {
                id: tabs
                model: root.backend ? root.backend.tabs : []

                delegate: Rectangle {
                    id: tab
                    required property var modelData
                    required property int index

                    width: Math.min(220, Math.max(126, tabLabel.implicitWidth + 46))
                    height: tabsRow.height
                    color: modelData.active
                           ? (root.theme ? root.theme.surface : "#151820")
                           : (tabMouse.containsMouse
                              ? (root.theme ? root.theme.hover : "#202531")
                              : "transparent")

                    Behavior on color {
                        ColorAnimation { duration: 140; easing.type: Easing.OutCubic }
                    }

                    Rectangle {
                        anchors.top: parent.top
                        width: parent.width
                        height: 2
                        color: modelData.active
                               ? (root.theme ? root.theme.accent : "#8ab4f8")
                               : "transparent"
                    }

                    Rectangle {
                        anchors.left: parent.left
                        width: 1
                        height: parent.height - 12
                        anchors.verticalCenter: parent.verticalCenter
                        color: root.theme ? root.theme.border : "#292e3a"
                        visible: index > 0 && !modelData.active
                        opacity: 0.5
                    }

                    Text {
                        id: tabLabel
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        anchors.right: closeButton.left
                        anchors.rightMargin: 4
                        anchors.verticalCenter: parent.verticalCenter
                        text: modelData.title
                              + (modelData.searching ? "  \u2315" : "")
                              + (modelData.loading ? "  \u2026" : "")
                        elide: Text.ElideMiddle
                        color: modelData.active
                               ? (root.theme ? root.theme.foreground : "#e7e9ee")
                               : (root.theme ? root.theme.muted : "#9299a8")
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                        font.weight: modelData.active ? Font.DemiBold : Font.Normal
                    }

                    Item {
                        id: closeButton
                        width: 22
                        height: parent.height
                        anchors.right: parent.right
                        visible: modelData.active || tabMouse.containsMouse

                        Text {
                            anchors.centerIn: parent
                            text: "\u00d7"
                            color: closeMouse.containsMouse
                                   ? (root.theme ? root.theme.foreground : "#e7e9ee")
                                   : (root.theme ? root.theme.muted : "#9299a8")
                            font.pixelSize: 15 * (theme ? theme.font_scale : 1)
                        }

                        MouseArea {
                            id: closeMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.backend.close_tab(index)
                        }
                    }

                    MouseArea {
                        id: tabMouse
                        anchors.fill: parent
                        anchors.rightMargin: closeButton.width
                        hoverEnabled: true
                        acceptedButtons: Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
                        cursorShape: Qt.PointingHandCursor

                        property real pressX: 0

                        onPressed: function(mouse) {
                            pressX = mouse.x
                            if (mouse.button === Qt.MiddleButton)
                                root.backend.close_tab(index)
                        }

                        onClicked: function(mouse) {
                            if (mouse.button === Qt.RightButton) {
                                root.menuTab = index
                                menu.entries = root.tabMenuEntries(index)
                                var point = mapToItem(root, mouse.x, mouse.y)
                                menu.open(point.x, point.y)
                                return
                            }
                            if (mouse.button === Qt.LeftButton)
                                root.backend.select_tab(index)
                        }

                        onReleased: function(mouse) {
                            if (mouse.button !== Qt.LeftButton)
                                return
                            if (Math.abs(mouse.x - pressX) < 26)
                                return
                            var point = mapToItem(tabsRow, mouse.x, mouse.y)
                            var target = root.indexAt(point.x)
                            if (target >= 0 && target !== index)
                                root.backend.move_tab(index, target)
                        }
                    }

                    DropArea {
                        anchors.fill: parent
                        z: 3
                        keys: ["text/uri-list", "filem/paths"]
                        onEntered: {
                            root.dropHoverTab = index
                            dropHoverTimer.restart()
                        }
                        onExited: {
                            if (root.dropHoverTab === index) {
                                dropHoverTimer.stop()
                                root.dropHoverTab = -1
                            }
                        }
                        onDropped: function(drop) {
                            dropHoverTimer.stop()
                            root.dropHoverTab = -1
                            root.acceptDrop(drop, modelData.path)
                        }
                    }
                }
            }

            Item {
                width: 34
                height: tabsRow.height

                Rectangle {
                    width: 24
                    height: 24
                    anchors.centerIn: parent
                    radius: root.theme ? Math.max(3, root.theme.radius * 0.45) : 5
                    color: plusMouse.containsMouse
                           ? (root.theme ? root.theme.hover : "#202531")
                           : "transparent"

                    Text {
                        anchors.centerIn: parent
                        text: "+"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.pixelSize: 17 * (theme ? theme.font_scale : 1)
                    }

                    MouseArea {
                        id: plusMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.backend.new_tab("")
                    }
                }
            }
        }
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: theme ? theme.border : "#292e3a"
    }

    ContextMenu {
        id: menu
        theme: root.theme
        entries: []
    }

    Connections {
        target: root.backend
        function onTabs_changed() {
            if (root.backend)
                Qt.callLater(function() {
                    root.ensureVisible(root.backend.active_tab)
                })
        }
    }
}
