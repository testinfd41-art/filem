import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root

    property var theme
    property var backend
    property int panelWidth: 220
    property alias placesView: placesList
    property bool tagsExpanded: true
    property var selectedTagIndices: []
    property string dropHoverPath: ""

    signal bookmarkRenameRequested(int index, string label)
    signal emptyTrashRequested()
    signal unlockRequested(string device, string label)
    signal ejectRequested(string path, string device, string label)
    signal networkLocationRequested()

    width: backend && backend.sidebar_visible ? panelWidth : 0
    visible: width > 0
    clip: true
    color: theme ? theme.surface : "#151820"

    Behavior on width {
        NumberAnimation { duration: 170; easing.type: Easing.OutCubic }
    }

    Component.onCompleted: {
        if (backend) {
            panelWidth = backend.sidebar_width
            backend.refresh_devices()
            syncTagSelection()
        }
    }

    Timer {
        id: dropHoverTimer
        interval: root.backend ? Math.max(200, Math.min(5000, root.backend.drag_hover_delay_ms)) : 700
        repeat: false
        onTriggered: {
            var path = root.dropHoverPath
            root.dropHoverPath = ""
            if (root.backend && path !== "" && !path.startsWith("/.__filem_"))
                root.backend.navigate(path)
        }
    }

    Connections {
        target: root.backend
        function onBehavior_changed() {
            if (root.backend && root.backend.sidebar_width !== root.panelWidth)
                root.panelWidth = root.backend.sidebar_width
        }
        function onCurrent_path_changed() {
            root.syncTagSelection()
        }
    }

    function openPlace(index) {
        if (!backend || !backend.places)
            return
        var place = backend.places[index]
        if (place)
            backend.navigate(place.path)
    }

    function openBookmark(index) {
        if (!backend || !backend.bookmarks)
            return
        var bookmark = backend.bookmarks[index]
        if (bookmark)
            backend.navigate(bookmark.path)
    }

    function dropDestinationAt(x, y) {
        function rowAt(view) {
            if (!view || !view.visible)
                return -1
            var point = view.mapFromItem(root, x, y)
            if (point.x < 0 || point.x >= view.width || point.y < 0 || point.y >= view.height)
                return -1
            var contentYPos = point.y + view.contentY
            var idx = view.indexAt(view.width / 2, contentYPos)
            if (idx >= 0)
                return idx
            if (contentYPos >= 0) {
                var estimated = Math.floor(contentYPos / 31)
                if (estimated >= 0 && estimated < view.count)
                    return estimated
            }
            return -1
        }

        var index = rowAt(placesList)
        if (index >= 0 && root.backend && root.backend.places[index]) {
            var place = root.backend.places[index]
            if (place.id !== "recent" && place.id !== "trash"
                    && !String(place.path).startsWith("/.__filem_"))
                return place.path
        }

        index = rowAt(bookmarksList)
        if (index >= 0 && root.backend && root.backend.bookmarks[index])
            return root.backend.bookmarks[index].path

        index = rowAt(devicesList)
        if (index >= 0 && root.backend && root.backend.devices[index]) {
            var device = root.backend.devices[index]
            if (device.mounted && device.path)
                return device.path
        }
        return ""
    }

    function acceptDrop(drop, dest) {
        if (!root.backend || !dest || dest === "" || !drop || !drop.urls || !drop.urls.length)
            return false
        var urls = []
        for (var i = 0; i < drop.urls.length; i++)
            urls.push(drop.urls[i].toString())
        root.backend.drop_external_paths(dest, urls, drop.proposedAction === Qt.MoveAction)
        drop.acceptProposedAction()
        return true
    }

    function openTagFilter(index) {
        var selected = root.selectedTagIndices.slice()
        var position = selected.indexOf(index)
        if (position >= 0)
            selected.splice(position, 1)
        else
            selected.push(index)
        selected.sort(function(a, b) { return a - b })
        root.selectedTagIndices = selected
        root.backend.navigate("/.__filem_tags__/" + (selected.length ? selected.join(",") : "all"))
    }

    function openUntagged() {
        root.selectedTagIndices = []
        root.backend.navigate("/.__filem_tags__/untagged")
    }

    function syncTagSelection() {
        if (!root.backend)
            return
        var path = root.backend.current_path
        var prefix = "/.__filem_tags__/"
        if (!path.startsWith(prefix))
            return
        var route = path.substring(prefix.length)
        if (route === "all" || route === "untagged") {
            root.selectedTagIndices = []
            return
        }
        var selected = route.split(",").map(function(value) { return Number(value) })
        root.selectedTagIndices = selected.filter(function(value) {
            return Number.isInteger(value) && value >= 0 && value < 7
        })
    }

    function isBookmarked(path) {
        if (!backend || !backend.bookmarks)
            return false
        for (var i = 0; i < backend.bookmarks.length; i++) {
            if (backend.bookmarks[i].path === path)
                return true
        }
        return false
    }

    function placeGlyph(place) {
        if (place.id === "recent")
            return "recent"
        if (place.id === "home")
            return "home"
        if (place.id === "trash")
            return "trash"
        if (place.id === "desktop")
            return "desktop"
        if (place.id === "downloads")
            return "downloads"
        if (place.id === "documents")
            return "documents"
        if (place.id === "music")
            return "music"
        if (place.id === "pictures")
            return "pictures"
        if (place.id === "videos")
            return "videos"
        return "folder"
    }

    function deviceGlyph(device) {
        if (!device)
            return "hard-drive"
        if (device.system)
            return device.path === "/" ? "system-drive" : "hard-drive"
        if (device.network)
            return "network"
        var filesystem = (device.filesystem || "").toLowerCase()
        if (filesystem.indexOf("phone") >= 0 || filesystem.indexOf("apple") >= 0)
            return "smartphone"
        if (filesystem.indexOf("camera") >= 0)
            return "camera"
        if (device.encrypted)
            return "encrypted-drive"
        if (filesystem.indexOf("iso") >= 0 || filesystem.indexOf("udf") >= 0)
            return "disc"
        if (device.removable)
            return device.usbFlash ? "usb" : "external-drive"
        return "hard-drive"
    }

    function openDevice(device) {
        if (!backend || !device)
            return
        if (device.locked) {
            unlockRequested(device.device, device.label)
        } else if (device.mounted && device.path !== "") {
            backend.navigate(device.path)
        } else if (device.device !== "") {
            backend.mount_device(device.device)
        }
    }

    Rectangle {
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: 1
        color: theme ? theme.border : "#292e3a"
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 34

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 16
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 6
                text: "PLACES"
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
                font.letterSpacing: 1.2
            }

        }

        ListView {
            id: placesList
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(9, backend ? backend.places.length : 0) * 31 + 4
            clip: true
            spacing: 1
            model: backend ? backend.places : []
            currentIndex: -1
            keyNavigationWraps: false
            interactive: false
            keyNavigationEnabled: false

            delegate: Rectangle {
                id: placeRow
                required property var modelData
                required property int index

                x: 8
                width: ListView.view ? ListView.view.width - 16 : 0
                height: 30
                radius: theme ? Math.max(4, theme.radius * 0.55) : 6
                color: root.backend && root.backend.current_path === modelData.path
                       ? (theme ? theme.selection : "#24364f")
                       : (placeMouse.containsMouse || root.dropHoverPath === modelData.path
                          ? (theme ? theme.hover : "#202531") : "transparent")

                Behavior on color {
                    ColorAnimation { duration: 110; easing.type: Easing.OutCubic }
                }

                Row {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    anchors.rightMargin: 8
                    spacing: 9

                    ToolbarGlyph {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 16
                        height: 16
                        name: root.placeGlyph(modelData)
                        color: root.backend && root.backend.current_path === modelData.path
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (theme ? theme.muted : "#9299a8")
                    }

                    Text {
                        width: Math.max(0, parent.width - 25)
                        anchors.verticalCenter: parent.verticalCenter
                        text: modelData.label
                        elide: Text.ElideRight
                        color: root.backend && root.backend.current_path === modelData.path
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (theme ? theme.foreground : "#e7e9ee")
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                    }
                }

                MouseArea {
                    id: placeMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    cursorShape: Qt.PointingHandCursor

                    onClicked: function(mouse) {
                        placesList.currentIndex = index
                        if (mouse.button === Qt.RightButton) {
                            var point = mapToItem(root, mouse.x, mouse.y)
                            placeMenu.entries = [
                                { label: "Open", action: function() { root.backend.navigate(modelData.path) } },
                                { label: "Open in New Tab", action: function() { root.backend.new_tab(modelData.path) } }
                            ]
                            if (modelData.id === "recent") {
                                placeMenu.entries.push({ separator: true })
                                placeMenu.entries.push({
                                    label: "Clear Recent History",
                                    action: function() { root.backend.clear_recent_history() }
                                })
                            } else if (modelData.id === "trash") {
                                placeMenu.entries.push({ separator: true })
                                placeMenu.entries.push({
                                    label: "Empty Trash\u2026",
                                    danger: true,
                                    action: function() { root.emptyTrashRequested() }
                                })
                            } else {
                                placeMenu.entries.push({ separator: true })
                                placeMenu.entries.push({
                                    label: "Add to Bookmarks",
                                    action: function() { root.backend.add_bookmark(modelData.path, modelData.label) }
                                })
                            }
                            placeMenu.open(point.x, point.y)
                            return
                        }
                        root.backend.navigate(modelData.path)
                    }
                }

                DropArea {
                    anchors.fill: parent
                    z: 2
                    keys: ["text/uri-list", "filem/paths"]
                    onEntered: {
                        if (modelData.id !== "recent" && modelData.id !== "trash"
                                && !String(modelData.path).startsWith("/.__filem_")) {
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

            Keys.onPressed: function(event) {
                if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                    root.openPlace(placesList.currentIndex)
                    event.accepted = true
                } else if (event.key === Qt.Key_Escape) {
                    placesList.currentIndex = -1
                    event.accepted = true
                }
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 30

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 16
                anchors.verticalCenter: parent.verticalCenter
                text: root.tagsExpanded ? "\u2304" : "\u203a"
                color: theme ? theme.muted : "#9299a8"
                font.pixelSize: 13 * (theme ? theme.font_scale : 1)
            }
            Text {
                anchors.left: parent.left
                anchors.leftMargin: 31
                anchors.verticalCenter: parent.verticalCenter
                text: "TAGS"
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
                font.letterSpacing: 1.2
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.tagsExpanded = !root.tagsExpanded
            }

        }

        ListView {
            id: tagList
            Layout.fillWidth: true
            Layout.preferredHeight: root.tagsExpanded
                                    ? ((root.backend ? root.backend.tags.length : 0) + 2) * 28 + 2
                                    : 0
            clip: true
            spacing: 1
            visible: root.tagsExpanded
            model: root.backend ? root.backend.tags : []
            interactive: false
            keyNavigationEnabled: false

            delegate: Rectangle {
                required property var modelData
                required property int index
                x: 8
                width: ListView.view ? ListView.view.width - 16 : 0
                height: 27
                radius: theme ? Math.max(4, theme.radius * 0.55) : 6
                property string collectionPath: "/.__filem_tags__/" + modelData.index
                property bool activeTag: root.selectedTagIndices.indexOf(modelData.index) >= 0
                                         || (root.backend && root.backend.current_path === collectionPath)
                color: activeTag
                       ? (theme ? theme.selection : "#24364f")
                       : (tagMouse.containsMouse || dropArea.containsDrag
                          ? (theme ? theme.hover : "#202531") : "transparent")

                Rectangle {
                    anchors.left: parent.left
                    anchors.leftMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    width: 9
                    height: 9
                    radius: 5
                    color: modelData.color || (theme ? theme.tag_color(modelData.index) : "#9299a8")
                }
                Text {
                    anchors.left: parent.left
                    anchors.leftMargin: 28
                    anchors.right: countLabel.left
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData.name
                    elide: Text.ElideRight
                    color: parent.activeTag
                           ? (theme ? theme.selection_text : "#f2f5fb")
                           : (theme ? theme.foreground : "#e7e9ee")
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }
                Text {
                    id: countLabel
                    anchors.right: parent.right
                    anchors.rightMargin: 9
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData.count
                    color: theme ? theme.muted : "#9299a8"
                    font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                }
                MouseArea {
                    id: tagMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: function(mouse) {
                        if (mouse.modifiers & Qt.ControlModifier)
                            root.openTagFilter(modelData.index)
                        else {
                            root.selectedTagIndices = [modelData.index]
                            root.backend.navigate(parent.collectionPath)
                        }
                    }

                }
                DropArea {
                    id: dropArea
                    anchors.fill: parent
                    keys: ["text/uri-list", "filem/paths"]
                    onDropped: function(drop) {
                        var urls = drop.getDataAsString("text/uri-list") || ""
                        if (urls === "")
                            urls = drop.getDataAsString("filem/paths")
                        if (root.backend && urls !== "")
                            root.backend.tag_dropped_paths(urls, modelData.index)
                        drop.acceptProposedAction()
                    }
                }
            }

            footer: Column {
                x: 8
                width: tagList.width - 16
                spacing: 1
                Rectangle {
                    width: parent.width
                    height: 27
                    radius: theme ? Math.max(4, theme.radius * 0.55) : 6
                    property bool activeAll: root.backend
                                             && root.backend.current_path === "/.__filem_tags__/all"
                    color: activeAll
                           ? (theme ? theme.selection : "#24364f")
                           : (allMouse.containsMouse ? (theme ? theme.hover : "#202531") : "transparent")
                    Text {
                        anchors.left: parent.left
                        anchors.leftMargin: 28
                        anchors.verticalCenter: parent.verticalCenter
                        text: "All Tagged"
                        color: parent.activeAll
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (theme ? theme.foreground : "#e7e9ee")
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    }
                    MouseArea {
                        id: allMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.selectedTagIndices = []
                            root.backend.navigate("/.__filem_tags__/all")
                        }
                    }
                }
                Rectangle {
                    width: parent.width
                    height: 27
                    radius: theme ? Math.max(4, theme.radius * 0.55) : 6
                    property bool activeUntagged: root.backend
                                                  && root.backend.current_path === "/.__filem_tags__/untagged"
                    color: activeUntagged
                           ? (theme ? theme.selection : "#24364f")
                           : (untaggedMouse.containsMouse || untaggedDrop.containsDrag
                              ? (theme ? theme.hover : "#202531") : "transparent")
                    Text {
                        anchors.left: parent.left
                        anchors.leftMargin: 28
                        anchors.right: untaggedCount.left
                        anchors.verticalCenter: parent.verticalCenter
                        text: "Untagged"
                        color: parent.activeUntagged
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (theme ? theme.foreground : "#e7e9ee")
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    }
                    Text {
                        id: untaggedCount
                        anchors.right: parent.right
                        anchors.rightMargin: 9
                        anchors.verticalCenter: parent.verticalCenter
                        text: root.backend ? root.backend.untagged_count : 0
                        color: theme ? theme.muted : "#9299a8"
                        font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                    }
                    MouseArea {
                        id: untaggedMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.openUntagged()
                    }
                    DropArea {
                        id: untaggedDrop
                        anchors.fill: parent
                        keys: ["text/uri-list", "filem/paths"]
                        onDropped: function(drop) {
                            var urls = drop.getDataAsString("text/uri-list") || ""
                            if (urls === "")
                                urls = drop.getDataAsString("filem/paths")
                            if (root.backend && urls !== "")
                                root.backend.tag_dropped_paths(urls, -1)
                            drop.acceptProposedAction()
                        }
                    }
                }
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 30

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 16
                anchors.verticalCenter: parent.verticalCenter
                text: "BOOKMARKS"
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
                font.letterSpacing: 1.2
            }

        }

        ListView {
            id: bookmarksList
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(7, backend ? backend.bookmarks.length : 0) * 31 + 4
            Layout.bottomMargin: 6
            clip: true
            spacing: 1
            model: backend ? backend.bookmarks : []
            currentIndex: -1
            interactive: false
            keyNavigationEnabled: false

            delegate: Rectangle {
                required property var modelData
                required property int index

                x: 8
                width: ListView.view ? ListView.view.width - 16 : 0
                height: 30
                radius: theme ? Math.max(4, theme.radius * 0.55) : 6
                color: root.backend && root.backend.current_path === modelData.path
                       ? (theme ? theme.selection : "#24364f")
                       : (bookmarkMouse.containsMouse || root.dropHoverPath === modelData.path
                          ? (theme ? theme.hover : "#202531") : "transparent")

                Behavior on color {
                    ColorAnimation { duration: 110; easing.type: Easing.OutCubic }
                }

                Row {
                    anchors.fill: parent
                    anchors.leftMargin: 9
                    anchors.rightMargin: 8
                    spacing: 8

                    ToolbarGlyph {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 16
                        height: 16
                        name: "bookmark"
                        color: root.backend && root.backend.current_path === modelData.path
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (theme ? theme.muted : "#9299a8")
                    }

                    Text {
                        width: Math.max(0, parent.width - 20)
                        anchors.verticalCenter: parent.verticalCenter
                        text: modelData.label
                        elide: Text.ElideRight
                        color: root.backend && root.backend.current_path === modelData.path
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (theme ? theme.foreground : "#e7e9ee")
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    }
                }

                MouseArea {
                    id: bookmarkMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    cursorShape: Qt.PointingHandCursor

                    onClicked: function(mouse) {
                        bookmarksList.currentIndex = index
                        if (mouse.button === Qt.RightButton) {
                            var point = mapToItem(root, mouse.x, mouse.y)
                            bookmarkMenu.entries = [
                                { label: "Open", action: function() { root.backend.navigate(modelData.path) } },
                                { label: "Open in New Tab", action: function() { root.backend.new_tab(modelData.path) } },
                                { separator: true },
                                { label: "Rename\u2026", action: function() { root.bookmarkRenameRequested(index, modelData.label) } },
                                { label: "Move Up", enabled: index > 0, action: function() { root.backend.move_bookmark(index, -1) } },
                                { label: "Move Down", enabled: index < root.backend.bookmarks.length - 1, action: function() { root.backend.move_bookmark(index, 1) } },
                                { separator: true },
                                { label: "Remove Bookmark", danger: true, action: function() { root.backend.remove_bookmark(index) } }
                            ]
                            bookmarkMenu.open(point.x, point.y)
                            return
                        }
                        root.backend.navigate(modelData.path)
                    }
                }

                DropArea {
                    anchors.fill: parent
                    z: 2
                    keys: ["text/uri-list", "filem/paths"]
                    onEntered: {
                        root.dropHoverPath = modelData.path
                        dropHoverTimer.restart()
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

            Keys.onPressed: function(event) {
                if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                    root.openBookmark(bookmarksList.currentIndex)
                    event.accepted = true
                } else if (event.key === Qt.Key_Escape) {
                    bookmarksList.currentIndex = -1
                    event.accepted = true
                }
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 32

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 16
                anchors.verticalCenter: parent.verticalCenter
                text: "DEVICES"
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
                font.letterSpacing: 1.2
            }

            Item {
                width: 104
                height: 28
                anchors.right: parent.right
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter

                Rectangle {
                    anchors.left: parent.left
                    anchors.leftMargin: 2
                    anchors.verticalCenter: parent.verticalCenter
                    width: 26
                    height: 26
                    radius: theme ? Math.max(3, theme.radius * 0.45) : 5
                    color: refreshMouse.containsMouse ? (theme ? theme.hover : "#202531") : "transparent"

                    ToolbarGlyph {
                        anchors.centerIn: parent
                        width: 16
                        height: 16
                        name: "refresh"
                        color: root.backend && root.backend.devices_busy
                               ? (theme ? theme.accent : "#8ab4f8")
                               : (theme ? theme.muted : "#9299a8")
                    }

                    MouseArea {
                        id: refreshMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: if (root.backend) root.backend.refresh_devices()
                    }
                }

                Rectangle {
                    anchors.right: parent.right
                    anchors.rightMargin: 2
                    anchors.verticalCenter: parent.verticalCenter
                    width: 26
                    height: 26
                    radius: theme ? Math.max(3, theme.radius * 0.45) : 5
                    color: plusMouse.containsMouse ? (theme ? theme.selection : "#24364f") : "transparent"

                    ToolbarGlyph {
                        anchors.centerIn: parent
                        width: 16
                        height: 16
                        name: "plus"
                        color: plusMouse.containsMouse
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (theme ? theme.muted : "#9299a8")
                    }

                    MouseArea {
                        id: plusMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.networkLocationRequested()
                    }
                }

                DeviceGlyph {
                    anchors.right: parent.right
                    anchors.rightMargin: 34
                    anchors.verticalCenter: parent.verticalCenter
                    width: 16
                    height: 16
                    name: "network"
                    color: plusMouse.containsMouse || networkMouse.containsMouse
                           ? (theme ? theme.foreground : "#e7e9ee")
                           : (theme ? theme.muted : "#9299a8")
                }

                MouseArea {
                    id: networkMouse
                    anchors.right: parent.right
                    anchors.rightMargin: 30
                    anchors.verticalCenter: parent.verticalCenter
                    width: 24
                    height: 28
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.networkLocationRequested()
                }
            }
        }

        ListView {
            id: devicesList
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: Math.min(4, backend ? backend.devices.length : 0) * 56 + 4
            clip: true
            spacing: 1
            model: backend ? backend.devices : []
            currentIndex: -1
            interactive: false
            keyNavigationEnabled: false

            delegate: Rectangle {
                required property var modelData
                required property int index
                x: 8
                width: ListView.view ? ListView.view.width - 16 : 0
                height: 54
                radius: theme ? Math.max(4, theme.radius * 0.55) : 6
                color: root.backend && modelData.mounted
                       && root.backend.current_path === modelData.path
                       ? (theme ? theme.selection : "#24364f")
                       : (deviceMouse.containsMouse || deviceDrop.containsDrag
                          || root.dropHoverPath === modelData.path
                          ? (theme ? theme.hover : "#202531") : "transparent")
                opacity: root.backend && root.backend.devices_busy ? 0.7 : 1

                Row {
                    anchors.fill: parent
                    anchors.leftMargin: 10
                    anchors.rightMargin: 8
                    spacing: 10

                    DeviceGlyph {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 20
                        height: 20
                        name: root.deviceGlyph(modelData)
                        color: root.backend && modelData.mounted
                               && root.backend.current_path === modelData.path
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (modelData.mounted
                                  ? (theme ? theme.foreground : "#e7e9ee")
                                  : (theme ? theme.muted : "#9299a8"))
                        opacity: modelData.locked ? 0.85 : 1
                    }

                    Column {
                        anchors.verticalCenter: parent.verticalCenter
                        width: Math.max(0, parent.width - 30)
                        spacing: 3
                        Text {
                            width: parent.width
                            text: modelData.label
                            elide: Text.ElideRight
                            color: root.backend && modelData.mounted
                                   && root.backend.current_path === modelData.path
                                   ? (theme ? theme.selection_text : "#f2f5fb")
                                   : (theme ? theme.foreground : "#e7e9ee")
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                        }
                        Text {
                            width: parent.width
                            text: root.backend && root.backend.devices_busy
                                  && root.backend.device_action === modelData.device
                                  ? "Connecting\u2026"
                                  : (modelData.locked
                                     ? "Locked" + (modelData.capacity ? " \u00b7 " + modelData.capacity : "")
                                     : (modelData.mounted
                                        ? "Mounted \u00b7 " + (modelData.system
                                           ? [modelData.path, modelData.filesystem, modelData.capacity]
                                           : [modelData.filesystem, modelData.capacity]).filter(function(value) { return value !== "" }).join(" \u00b7 ")
                                        : "Not mounted" + (modelData.capacity ? " \u00b7 " + modelData.capacity : "")))
                            elide: Text.ElideRight
                            color: theme ? theme.muted : "#9299a8"
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: 9 * (theme ? theme.font_scale : 1)
                        }

                        Rectangle {
                            width: parent.width
                            height: 3
                            radius: 2
                            visible: modelData.mounted && modelData.capacity !== ""
                            color: theme ? theme.border : "#292e3a"

                            Rectangle {
                                width: parent.width * Math.max(0, Math.min(1, modelData.capacityRatio))
                                height: parent.height
                                radius: parent.radius
                                color: modelData.capacityRatio >= 0.95
                                       ? (theme ? theme.danger : "#e06c75")
                                       : (theme ? theme.accent : "#8ab4f8")
                            }
                        }
                    }
                }

                MouseArea {
                    id: deviceMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    cursorShape: Qt.PointingHandCursor
                    enabled: !root.backend || !root.backend.devices_busy
                    onClicked: function(mouse) {
                        if (mouse.button === Qt.RightButton) {
                            var point = mapToItem(root, mouse.x, mouse.y)
                            var entries = []
                            if (modelData.locked) {
                                entries.push({ label: "Unlock\u2026", action: function() { root.unlockRequested(modelData.device, modelData.label) } })
                            } else if (modelData.mounted) {
                                entries.push({ label: "Open", action: function() { root.backend.navigate(modelData.path) } })
                                if (modelData.canUnmount)
                                    entries.push({ label: "Unmount", danger: true, action: function() { root.backend.unmount_device(modelData.path) } })
                                if (modelData.canEject)
                                    entries.push({ label: "Eject", danger: true, action: function() { root.ejectRequested(modelData.path, modelData.ejectDevice, modelData.label) } })
                            } else {
                                entries.push({ label: "Mount", action: function() { root.backend.mount_device(modelData.device) } })
                            }
                            deviceMenu.entries = entries
                            deviceMenu.open(point.x, point.y)
                        } else {
                            root.openDevice(modelData)
                        }
                    }

                    DropArea {
                        id: deviceDrop
                        anchors.fill: parent
                        z: 2
                        keys: ["text/uri-list", "filem/paths"]
                        onEntered: {
                            if (modelData.mounted) {
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
                            if (modelData.mounted)
                                root.acceptDrop(drop, modelData.path)
                        }
                    }
                }
            }
        }

    }

    // Resize handle. Width is committed once, on release, so config writes do
    // not happen for every pixel of dragging.
    Rectangle {
        id: resizeHandle
        width: 4
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        color: resizeMouse.pressed
               ? (theme ? theme.accent : "#8ab4f8")
               : (resizeMouse.containsMouse ? (theme ? theme.border : "#292e3a") : "transparent")
        z: 10

        MouseArea {
            id: resizeMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.SizeHorCursor
            preventStealing: true

            onPositionChanged: function(mouse) {
                if (!pressed)
                    return
                var point = mapToItem(root, mouse.x, 0)
                root.panelWidth = Math.max(160, Math.min(420, point.x))
            }

            onReleased: {
                if (root.backend)
                    root.backend.set_sidebar_width(root.panelWidth)
            }
        }
    }

    ContextMenu {
        id: placeMenu
        theme: root.theme
        entries: []
    }

    ContextMenu {
        id: bookmarkMenu
        theme: root.theme
        entries: []
    }

    ContextMenu {
        id: deviceMenu
        theme: root.theme
        entries: []
    }
}
