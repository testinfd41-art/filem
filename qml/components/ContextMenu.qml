pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import "MenuManager.js" as MenuManager

/// A small themed menu. Entries are plain objects:
/// { label, shortcut, danger, enabled, separator, action }.
FocusScope {
    id: root
    objectName: "filemContextMenu"

    property bool opened: false
    visible: opened || opacity > 0
    opacity: opened ? 1 : 0
    scale: opened ? 1 : 0.98
    focus: true
    z: 300
    width: 252
    height: panel.height

    property var theme
    property var entries: []
    property int currentIndex: -1
    property real anchorX: 0
    property real anchorY: 0
    property var menuWindow: Window.window
    property var previousFocusItem: null
    readonly property alias scrollViewport: menuViewport
    // The component that owns this menu. Kept after re-parenting so caller
    // coordinates can always be mapped from the same origin.
    property Item home: null

    signal dismissed()

    Behavior on opacity {
        NumberAnimation { duration: 110; easing.type: Easing.OutCubic }
    }

    Behavior on scale {
        NumberAnimation { duration: 130; easing.type: Easing.OutCubic }
    }

    function enabledIndex(start, step) {
        if (entries.length === 0)
            return -1
        var index = start
        for (var count = 0; count < entries.length; count++) {
            index = (index + step + entries.length) % entries.length
            var entry = entries[index]
            if (entry && !entry.separator && entry.enabled !== false)
                return index
        }
        return -1
    }

    function moveSelection(step) {
        currentIndex = enabledIndex(currentIndex, step)
        if (currentIndex < 0)
            return
        var targetY = 0
        for (var i = 0; i < currentIndex; i++)
            targetY += entries[i] && entries[i].separator ? 9 : 30
        if (targetY < menuViewport.contentY)
            menuViewport.contentY = targetY
        else if (targetY + (entries[currentIndex] && entries[currentIndex].separator ? 9 : 30)
                 > menuViewport.contentY + menuViewport.height)
            menuViewport.contentY = targetY + 30 - menuViewport.height
    }

    function open(px, py) {
        if (!home)
            home = parent
        menuWindow = Window.window
        if (!previousFocusItem)
            previousFocusItem = menuWindow && menuWindow.activeFocusItem ? menuWindow.activeFocusItem : null
        var target = menuWindow && menuWindow.contentItem ? menuWindow.contentItem : null
        var scene = home && target ? home.mapToItem(target, px, py) : Qt.point(px, py)
        if (target && root.parent !== target)
            root.parent = target
        anchorX = scene.x
        anchorY = scene.y
        opened = true
        currentIndex = enabledIndex(-1, 1)
        MenuManager.registerMenu(root)
        place()
        root.forceActiveFocus()
        Qt.callLater(function() {
            if (root.opened) {
                root.place()
                root.forceActiveFocus()
            }
        })
    }

    function place() {
        var bounds = menuWindow && menuWindow.contentItem ? menuWindow.contentItem : parent
        if (!bounds)
            return
        var margin = 8
        var anchorX = Math.max(margin, Math.min(root.anchorX, bounds.width - margin))
        var anchorY = Math.max(margin, Math.min(root.anchorY, bounds.height - margin))
        var availableWidth = Math.max(1, bounds.width - margin * 2)
        var availableHeight = Math.max(1, bounds.height - margin * 2)
        var desiredWidth = 252
        var desiredHeight = column.implicitHeight + 12
        width = Math.min(desiredWidth, availableWidth)
        panel.width = width
        var rightSpace = Math.max(0, bounds.width - margin - anchorX)
        var leftSpace = Math.max(0, anchorX - margin)
        if (rightSpace >= width)
            x = anchorX
        else if (leftSpace >= width)
            x = anchorX - width
        else
            x = Math.max(margin, Math.min(anchorX, bounds.width - margin - width))

        var belowSpace = Math.max(0, bounds.height - margin - anchorY)
        var aboveSpace = Math.max(0, anchorY - margin)
        var popupHeight = Math.min(desiredHeight, availableHeight)
        if (belowSpace >= desiredHeight)
            y = anchorY
        else if (aboveSpace >= desiredHeight)
            y = anchorY - popupHeight
        else if (belowSpace >= aboveSpace)
            y = Math.max(margin, Math.min(anchorY, bounds.height - margin - popupHeight))
        else
            y = Math.max(margin, anchorY - popupHeight)

        panel.height = popupHeight
    }

    function close() {
        if (!opened)
            return
        opened = false
        MenuManager.unregisterMenu(root)
        var restoreFocus = previousFocusItem
        previousFocusItem = null
        if (MenuManager.hasOpenMenu()) {
            var top = MenuManager.topMenu()
            if (top)
                top.forceActiveFocus()
        } else if (restoreFocus) {
            Qt.callLater(function() {
                if (restoreFocus && typeof restoreFocus.forceActiveFocus === "function")
                    restoreFocus.forceActiveFocus()
            })
        }
        dismissed()
    }

    function choose(index) {
        if (index < 0 || index >= entries.length)
            return
        var entry = entries[index]
        if (!entry || entry.separator || entry.enabled === false)
            return
        if (typeof entry.action === "function")
            entry.action()
        close()
    }

    Rectangle {
        id: panel
        width: root.width
        height: Math.min(column.implicitHeight + 12, Math.max(1, root.parent ? root.parent.height - 16 : column.implicitHeight + 12))
        radius: root.theme ? Math.max(5, root.theme.radius * 0.7) : 8
        color: root.theme ? root.theme.surface : "#151820"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        Flickable {
            id: menuViewport
            x: 0
            y: 6
            width: parent.width
            height: Math.max(1, parent.height - 12)
            clip: true
            contentWidth: width
            contentHeight: column.implicitHeight
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: column
                width: menuViewport.width

                Repeater {
                    model: root.entries

                    delegate: Rectangle {
                        id: row
                        required property var modelData
                        required property int index

                        width: menuViewport.width
                        height: modelData && modelData.separator ? 9 : 30
                        color: !modelData.separator && index === root.currentIndex
                               ? (root.theme ? root.theme.selection : "#24364f")
                               : "transparent"

                        Rectangle {
                            visible: row.modelData && row.modelData.separator === true
                            anchors.centerIn: parent
                            width: parent.width - 20
                            height: 1
                            color: root.theme ? root.theme.border : "#292e3a"
                        }

                        Row {
                            visible: !(row.modelData && row.modelData.separator === true)
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 12
                            spacing: 12
                            opacity: row.modelData && row.modelData.enabled === false ? 0.45 : 1.0

                            Text {
                                width: parent.width - shortcut.width - 12
                                anchors.verticalCenter: parent.verticalCenter
                                text: row.modelData && row.modelData.label ? row.modelData.label : ""
                                elide: Text.ElideRight
                                color: row.modelData && row.modelData.danger
                                       ? (root.theme ? root.theme.danger : "#e06c75")
                                       : (row.index === root.currentIndex
                                          ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                                          : (root.theme ? root.theme.foreground : "#e7e9ee"))
                                font.family: root.theme ? root.theme.font_family : "sans-serif"
                                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                            }

                            Text {
                                id: shortcut
                                anchors.verticalCenter: parent.verticalCenter
                                text: row.modelData && row.modelData.shortcut ? row.modelData.shortcut : ""
                                color: root.theme ? root.theme.muted : "#9299a8"
                                font.family: root.theme ? root.theme.mono_font : "monospace"
                                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            enabled: row.modelData && !row.modelData.separator && row.modelData.enabled !== false
                            onEntered: root.currentIndex = row.index
                            onClicked: root.choose(row.index)
                        }
                    }
                }
            }
        }
    }

    // Clicking anywhere else dismisses the menu without activating the list.
    MouseArea {
        parent: root.menuWindow && root.menuWindow.contentItem
                ? root.menuWindow.contentItem : root.parent
        anchors.fill: parent
        visible: root.opened
        z: root.z - 1
        onPressed: root.close()
    }

    // Focus-loss auto-close is intentionally omitted. The click-outside
    // MouseArea, Keys.onEscapePressed and the global Escape shortcut handle
    // all dismissal paths.  Auto-closing on focus loss races with the
    // re-parenting inside open() and prevents the Escape key from reaching
    // Keys.onEscapePressed.

    Component.onCompleted: home = parent
    onEntriesChanged: if (opened) Qt.callLater(place)

    Connections {
        target: root.menuWindow
        function onWidthChanged() {
            if (root.opened)
                Qt.callLater(function() { root.place() })
        }
        function onHeightChanged() {
            if (root.opened)
                Qt.callLater(function() { root.place() })
        }
    }

    Connections {
        target: root.menuWindow ? root.menuWindow.contentItem : null
        function onWidthChanged() {
            if (root.opened)
                Qt.callLater(function() { root.place() })
        }
        function onHeightChanged() {
            if (root.opened)
                Qt.callLater(function() { root.place() })
        }
    }

    Connections {
        target: column
        function onImplicitHeightChanged() {
            if (root.opened)
                Qt.callLater(function() { root.place() })
        }
    }

    Keys.onUpPressed: function(event) {
        root.moveSelection(-1)
        event.accepted = true
    }
    Keys.onDownPressed: function(event) {
        root.moveSelection(1)
        event.accepted = true
    }
    Keys.onReturnPressed: function(event) {
        root.choose(root.currentIndex)
        event.accepted = true
    }
    Keys.onEnterPressed: function(event) {
        root.choose(root.currentIndex)
        event.accepted = true
    }
    Keys.onEscapePressed: function(event) {
        root.close()
        event.accepted = true
    }
}
