import QtQuick

Rectangle {
    id: root

    property var theme
    property var backend
    property var area

    readonly property string entryPath: path
    readonly property bool selected: area ? area.isSelected(entryPath) : false
    readonly property bool hovered: rowMouse.containsMouse && !selected
    readonly property bool dragged: area && area.dragActive
                                    && area.dragPaths.indexOf(entryPath) >= 0
    readonly property bool focused: ListView.view
                                    && ListView.view.activeFocus
                                    && ListView.view.currentIndex === index
    readonly property bool cut: area ? area.isCut(entryPath) : false
    readonly property bool renaming: backend ? backend.renaming_path === entryPath : false
    readonly property bool treeEnabled: area && backend
                                        && backend.view_mode === "list"
                                        && !backend.search_mode
                                        && !backend.virtual_collection
    readonly property int treeIndent: treeEnabled ? Math.min(treeDepth, 16) * 14 : 0
    readonly property real listFontScale: root.area
                                          ? (root.area.zoom > 1
                                             ? 1 + (root.area.zoom - 1) * 0.1
                                             : root.area.zoom)
                                          : 1
    readonly property bool dragTarget: isDir && area
                                      && ((area.dragActive && area.dragHoverPath === entryPath)
                                          || rowDropArea.containsDrag)
    readonly property bool imageThumbnail: [
        "png", "jpg", "jpeg", "webp", "gif", "svg", "bmp", "tif", "tiff", "avif", "ico",
        "heic", "heif", "jxl", "tga", "jp2", "j2k"
    ].indexOf((extension || "").toLowerCase()) >= 0
    readonly property bool generatedThumbnail: (imageThumbnail
                                                && (extension || "").toLowerCase() !== "svg")
            || [
                "pdf", "mp4", "mkv", "webm", "avi", "mov", "m4v", "mpg", "mpeg", "ogv",
                "flv", "wmv", "3gp", "3g2", "ts", "mts", "m2ts", "vob",
                "mp3", "flac", "ogg", "oga", "opus", "m4a", "m4b", "wav", "wave", "aac",
                "wma", "aif", "aiff", "ape", "wv", "3ga"
            ].indexOf((extension || "").toLowerCase()) >= 0
    readonly property string thumbnailVersion: sizeText + "\u0000" + modifiedMs
    readonly property bool thumbnailInViewport: ListView.view
                                                && y + height > ListView.view.contentY
                                                && y < ListView.view.contentY + ListView.view.height
    property string thumbnailSource: ""

    function refreshThumbnail() {
        if (!backend || isDir)
            return
        if (imageThumbnail && (extension || "").toLowerCase() === "svg") {
            thumbnailSource = backend.file_url(entryPath)
        } else if (generatedThumbnail) {
            var cached = backend.thumbnail_url(entryPath)
            if (cached !== "") {
                setThumbnailSource(cached)
            } else {
                setThumbnailSource(iconUrl)
                if (thumbnailInViewport)
                    backend.request_thumbnail(entryPath, true)
            }
        } else {
            setThumbnailSource("")
        }
    }

    function setThumbnailSource(source) {
        if (thumbnailSource !== source)
            thumbnailSource = source
    }

    Component.onCompleted: refreshThumbnail()
    onEntryPathChanged: refreshThumbnail()
    onThumbnailVersionChanged: refreshThumbnail()
    onThumbnailInViewportChanged: {
        if (thumbnailInViewport && generatedThumbnail && thumbnailSource === iconUrl)
            refreshThumbnail()
    }

    Connections {
        target: root.backend
        function onThumbnail_ready_changed() {
            if (root.generatedThumbnail
                    && root.backend.thumbnail_ready_path === root.entryPath)
                root.refreshThumbnail()
        }
    }

    width: ListView.view ? ListView.view.width : 0
    height: root.area ? root.area.effectiveRowHeight : (theme ? theme.row_height : 34)
    opacity: cut ? 0.55 : 1.0
    color: dragTarget
           ? (theme ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, 0.22) : "#337ea7ff")
           : (selected
              ? (theme ? theme.selection : "#24364f")
              : "transparent")
    border.width: selected || focused || dragTarget || dragged ? 1 : 0
    border.color: dragged || selected
                  ? (theme ? theme.accent : "#8ab4f8")
                  : (theme ? theme.focus : "#8ab4f8")

    Rectangle {
        id: hoverOverlay
        anchors.fill: parent
        anchors.margins: 1
        color: theme ? theme.hover : "#202531"
        opacity: root.dragTarget || root.dragged || root.selected || !root.hovered ? 0 : 0.78

        Behavior on opacity {
            enabled: !root.selected
            NumberAnimation {
                duration: root.area ? root.area.hoverAnimationDuration : 100
                easing.type: Easing.OutCubic
            }
        }
    }

    Rectangle {
        anchors.fill: parent
        anchors.margins: 1
        color: theme ? theme.accent : "#8ab4f8"
        opacity: rowMouse.pressed && !root.dragTarget && !root.dragged ? 0.18 : 0
    }

    function open() {
        if (backend)
            backend.open_index(index)
    }

    function toggleTree() {
        if (!root.backend || !root.area || !treeExpandable)
            return
        root.area.activeView().currentIndex = index
        root.area.activeView().forceActiveFocus()
        root.backend.set_tree_path_expanded(root.entryPath, !treeExpanded)
    }

    // Narrow adjacent stripes show multiple tags without taking a full column.
    Row {
        width: tags ? Math.min(tags.length, 7) * 3 : 0
        height: parent.height
        Repeater {
            model: tags || []
            delegate: Rectangle {
                required property var modelData
                width: 3
                height: root.height
                color: theme ? theme.tag_color(modelData) : "transparent"
            }
        }
    }

    Row {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 8

        Item {
            width: root.area ? root.area.nameWidth(root.width) : Math.max(80, root.width - 360)
            height: parent.height

            Row {
                anchors.fill: parent
                spacing: 8

                Item {
                    width: root.treeIndent
                    height: parent.height
                }

                Item {
                    id: disclosureControl
                    width: root.treeEnabled && treeExpandable ? 14 : 0
                    height: parent.height
                    visible: width > 0

                    Text {
                        anchors.centerIn: parent
                        text: treeExpanded ? "\u2304" : "\u203a"
                        color: root.selected
                               ? (theme ? theme.selection_text : "#f2f5fb")
                               : (theme ? theme.muted : "#9299a8")
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 16 * root.listFontScale * (theme ? theme.font_scale : 1)
                    }

                }

                Image {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 24 * (root.area ? root.area.zoom : 1.0)
                    height: width
                    source: root.thumbnailSource || iconUrl
                    sourceSize.width: width
                    sourceSize.height: height
                    smooth: true
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: true
                    visible: source.toString() !== ""
                }

                Text {
                    id: fileNameText
                    visible: !root.renaming
                    width: Math.max(0, parent.width
                                    - (iconUrl !== "" ? 26 * (root.area ? root.area.zoom : 1.0) : 0)
                                    - root.treeIndent
                                    - (root.treeEnabled && treeExpandable ? 22 : 0))
                    anchors.verticalCenter: parent.verticalCenter
                    text: name + (isSymlink ? "  \u2192" : "")
                    elide: Text.ElideMiddle
                    color: root.selected
                           ? (theme ? theme.selection_text : "#f2f5fb")
                           : (theme ? theme.foreground : "#e7e9ee")
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 13 * root.listFontScale * (theme ? theme.font_scale : 1)
                    font.weight: isDir ? Font.Medium : Font.Normal
                }

                TextInput {
                    id: renameInput
                    visible: root.renaming
                    width: Math.max(0, parent.width
                                    - (iconUrl !== "" ? 26 * (root.area ? root.area.zoom : 1.0) : 0)
                                    - root.treeIndent
                                    - (root.treeEnabled && treeExpandable ? 22 : 0))
                    anchors.verticalCenter: parent.verticalCenter
                    text: name
                    color: theme ? theme.foreground : "#e7e9ee"
                    selectionColor: theme ? theme.selection : "#24364f"
                    selectedTextColor: theme ? theme.selection_text : "#f2f5fb"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 13 * root.listFontScale * (theme ? theme.font_scale : 1)
                    selectByMouse: true
                    clip: true
                    focus: root.renaming

                    function selectBase() {
                        forceActiveFocus()
                        var dot = text.lastIndexOf(".")
                        select(0, dot > 0 ? dot : text.length)
                    }

                    onVisibleChanged: {
                        if (visible)
                            selectBase()
                    }

                    onAccepted: {
                        if (root.backend)
                            root.backend.rename(root.entryPath, text)
                    }

                    Keys.onEscapePressed: function(event) {
                        if (root.backend)
                            root.backend.cancel_rename()
                        if (root.area)
                            root.area.listView.forceActiveFocus()
                        event.accepted = true
                    }
                }
            }
        }

        Text {
            visible: root.backend ? root.backend.show_size : true
            width: visible ? (root.area ? root.area.sizeWidth : 88) : 0
            height: parent.height
            horizontalAlignment: Text.AlignRight
            verticalAlignment: Text.AlignVCenter
            text: sizeText
            elide: Text.ElideRight
            color: root.selected
                   ? (theme ? theme.selection_text : "#f2f5fb")
                   : (theme ? theme.muted : "#9299a8")
            font.family: theme ? theme.mono_font : "monospace"
            font.pixelSize: 11 * root.listFontScale * (theme ? theme.font_scale : 1)
        }

        Text {
            visible: root.backend ? root.backend.show_modified : true
            width: visible ? (root.area ? root.area.modifiedWidth : 150) : 0
            height: parent.height
            verticalAlignment: Text.AlignVCenter
            text: modifiedMs > 0
                  ? Qt.formatDateTime(new Date(modifiedMs), "d MMM yyyy  HH:mm")
                  : "\u2014"
            elide: Text.ElideRight
            color: root.selected
                   ? (theme ? theme.selection_text : "#f2f5fb")
                   : (theme ? theme.muted : "#9299a8")
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 11 * root.listFontScale * (theme ? theme.font_scale : 1)
        }

        Text {
            visible: root.backend ? root.backend.show_kind : true
            width: visible ? (root.area ? root.area.kindWidth : 130) : 0
            height: parent.height
            verticalAlignment: Text.AlignVCenter
            // Search results and trashed items are identified by where they
            // live, which is more useful than repeating the file kind.
            text: root.backend && (root.backend.recent_view || root.backend.tag_collection_view)
                  ? kind + " \u00b7 " + displayPath
                  : (displayPath !== "" ? displayPath : kind)
            elide: Text.ElideMiddle
            color: root.selected
                   ? (theme ? theme.selection_text : "#f2f5fb")
                   : (theme ? theme.muted : "#9299a8")
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 11 * root.listFontScale * (theme ? theme.font_scale : 1)
        }
    }

    DropArea {
        id: rowDropArea
        anchors.fill: parent
        z: 2
        keys: ["text/uri-list", "filem/paths"]
        onEntered: {
            if (root.isDir && root.area) {
                if (root.area.dragActive)
                    root.area.setDragHoverPath(root.entryPath)
                root.area.beginFolderHover(root.entryPath)
            }
        }
        onExited: {
            if (root.area && root.area.dragHoverPath === root.entryPath)
                root.area.setDragHoverPath("")
            if (root.area)
                root.area.endFolderHover(root.entryPath)
        }
        onDropped: function(drop) {
            if (!root.area)
                return
            if (root.area.dragActive) {
                if (root.isDir
                        && root.area.dropDraggedPaths(root.entryPath, drop.proposedAction))
                    drop.acceptProposedAction()
            } else if (root.area.dropExternalPaths(
                           drop, root.isDir ? root.entryPath : root.backend.current_path)) {
                drop.acceptProposedAction()
            }
        }
    }

    function isRowEmptySpace(x) {
        var iconStart = 12 + root.treeIndent + (root.treeEnabled && treeExpandable ? 22 : 0)
        if (x < iconStart)
            return true
        var iconWidth = 24 * (root.area ? root.area.zoom : 1.0)
        var nameWidth = fileNameText ? fileNameText.paintedWidth : 120
        var iconAndNameEnd = iconStart + (iconUrl !== "" ? iconWidth + 8 : 0) + nameWidth + 8
        return x > iconAndNameEnd
    }

    MouseArea {
        id: rowMouse
        anchors.fill: parent
        // Put pointer tracking above the row visuals; the rename editor must
        // retain input ownership while it is active.
        z: root.renaming ? -1 : 1
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        cursorShape: Qt.ArrowCursor
        propagateComposedEvents: false

        property bool dragging: false
        property bool dragStarted: false
        property bool suppressClick: false
        property point dragOrigin: Qt.point(0, 0)
        property int pressModifiers: Qt.NoModifier
        property bool pressedSelected: false
        property bool isMarqueeGesture: false

        onPressed: function(mouse) {
            if (mouse.button !== Qt.LeftButton)
                return
            if (!root.area)
                return
            if (root.area.dragActive || root.area.selectingByMouse)
                return

            preventStealing = true
            dragging = true
            dragStarted = false
            suppressClick = false
            pressModifiers = mouse.modifiers
            dragOrigin = Qt.point(mouse.x, mouse.y)
            pressedSelected = root.area.isSelected(root.entryPath)
            isMarqueeGesture = !pressedSelected && !(mouse.modifiers & Qt.ShiftModifier) && root.isRowEmptySpace(mouse.x)

            root.area.activeView().currentIndex = index
            root.area.activeView().forceActiveFocus()
            root.area.setDragHoverPath("")

            if (!isMarqueeGesture) {
                if (mouse.modifiers & Qt.ShiftModifier)
                    root.area.extendSelection(index)
                else if (mouse.modifiers & Qt.ControlModifier)
                    root.area.toggleSelection(root.entryPath, index)
                else if (!pressedSelected)
                    root.area.setSingleSelection(root.entryPath, index)
            }
        }

        onPositionChanged: function(mouse) {
            if (!dragging || !root.area)
                return
            var moved = Math.abs(mouse.x - dragOrigin.x) + Math.abs(mouse.y - dragOrigin.y)
            if (moved <= Qt.styleHints.startDragDistance)
                return

            suppressClick = true

            if (isMarqueeGesture) {
                if (!dragStarted) {
                    dragStarted = true
                    var originPoint = mapToItem(root.area.activeView(), dragOrigin.x, dragOrigin.y)
                    root.area.beginMouseSelection(root.area.gridMode ? "grid" : root.area.columnMode ? "column" : "list", originPoint.x, originPoint.y, pressModifiers)
                }
                var currentPoint = mapToItem(root.area.activeView(), mouse.x, mouse.y)
                root.area.updateMouseSelectionPointer(currentPoint.x, currentPoint.y)
                return
            }

            if (!dragStarted) {
                dragStarted = true
                var paths = root.area.selectedPaths()
                if (paths.indexOf(root.entryPath) < 0)
                    paths = [root.entryPath]
                root.area.beginDrag(paths, pressModifiers, root.iconUrl)
            }

            if (root.area.dragActive) {
                var areaPoint = mapToItem(root.area, mouse.x, mouse.y)
                root.area.updateDragPointer(areaPoint.x, areaPoint.y)
            }
        }

        onReleased: function(mouse) {
            preventStealing = false
            if (!dragging)
                return
            dragging = false

            if (isMarqueeGesture) {
                if (dragStarted) {
                    dragStarted = false
                    isMarqueeGesture = false
                    root.area.finishMouseSelection()
                    dragOrigin = Qt.point(0, 0)
                    return
                }
                isMarqueeGesture = false
                dragOrigin = Qt.point(0, 0)
                // Click on empty row space (no drag) → clear selection
                if (!suppressClick && !(mouse.modifiers & (Qt.ControlModifier | Qt.ShiftModifier))) {
                    root.area.clearSelection()
                    root.area.activeView().currentIndex = -1
                    root.area.activeView().forceActiveFocus()
                }
                return
            }

            if (dragStarted) {
                dragStarted = false
                if (root.area && root.area.dragActive) {
                    var areaPoint = mapToItem(root.area, mouse.x, mouse.y)
                    root.area.finishInternalDrag(areaPoint.x, areaPoint.y)
                }
                dragOrigin = Qt.point(0, 0)
                return
            }

            dragOrigin = Qt.point(0, 0)

            if (pressedSelected && !suppressClick) {
                if (mouse.modifiers & Qt.ShiftModifier)
                    root.area.extendSelection(index)
                else if (mouse.modifiers & Qt.ControlModifier)
                    root.area.toggleSelection(root.entryPath, index)
                else
                    root.area.setSingleSelection(root.entryPath, index)
            }
        }

        onCanceled: {
            preventStealing = false
            dragging = false
            if (isMarqueeGesture) {
                if (dragStarted && root.area)
                    root.area.finishMouseSelection()
                dragStarted = false
                isMarqueeGesture = false
            } else if (dragStarted) {
                dragStarted = false
                if (root.area)
                    root.area.finishDrag()
            }
            dragOrigin = Qt.point(0, 0)
        }

        onClicked: function(mouse) {
            if (suppressClick) {
                suppressClick = false
                return
            }

            var disclosurePoint = mapToItem(disclosureControl, mouse.x, mouse.y)
            if (disclosureControl.visible
                    && disclosurePoint.x >= 0 && disclosurePoint.x < disclosureControl.width
                    && disclosurePoint.y >= 0 && disclosurePoint.y < disclosureControl.height) {
                root.toggleTree()
                return
            }
            if (!root.area)
                return

            root.area.activeView().currentIndex = index
            root.area.activeView().forceActiveFocus()

            if (mouse.button === Qt.RightButton) {
                mouse.accepted = true
                if (!root.area.isSelected(root.entryPath))
                    root.area.setSingleSelection(root.entryPath, index)
                root.area.openItemMenu(index, mapToItem(root.area, mouse.x, mouse.y))
                return
            }

            if (root.backend && root.backend.single_click_open
                    && mouse.modifiers === Qt.NoModifier)
                root.open()
        }

        onDoubleClicked: function(mouse) {
            if (mouse.button !== Qt.LeftButton)
                return
            var disclosurePoint = mapToItem(disclosureControl, mouse.x, mouse.y)
            if (disclosureControl.visible
                    && disclosurePoint.x >= 0 && disclosurePoint.x < disclosureControl.width
                    && disclosurePoint.y >= 0 && disclosurePoint.y < disclosureControl.height)
                return
            if (root.area) {
                root.area.activeView().currentIndex = index
                root.area.activeView().forceActiveFocus()
            }
            root.open()
        }
    }

    Accessible.role: Accessible.ListItem
    Accessible.name: name
    Accessible.description: displayPath !== "" ? displayPath : kind
}
