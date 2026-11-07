import QtQuick

Item {
    id: root

    property var theme
    property var backend
    property var externalDropResolver: null

    readonly property var zoomLevels: [0.5, 0.75, 1.0, 1.5, 2.0, 2.5]
    property real zoom: 1.0
    readonly property real gridIconSize: 100 * zoom
    readonly property real gridLabelFontSize: Math.max(10, 12 * Math.pow(zoom, 0.15))
    readonly property real gridLabelHeight: Math.ceil(gridLabelFontSize * 1.4
                                                      * (backend && (backend.recent_view || backend.tag_collection_view) ? 5 : 3) + 4)
    readonly property real gridBaseCellWidth: gridIconSize + 36 * zoom
    readonly property int gridColumnCount: Math.max(1, Math.floor(gridView.width / gridBaseCellWidth))
    readonly property real gridCellWidth: gridView.width / gridColumnCount
    readonly property real gridCellHeight: gridIconSize + gridLabelHeight + 10
    readonly property real effectiveRowHeight: Math.max(
        (theme ? theme.row_height : 34) * zoom,
        13 * zoom * 1.5 + 12,
        11 * zoom * 1.5 + 12
    )

    function zoomLevelIndex(value) {
        var closest = 0
        var distance = Math.abs(root.zoomLevels[0] - value)
        for (var i = 1; i < root.zoomLevels.length; i++) {
            var nextDistance = Math.abs(root.zoomLevels[i] - value)
            if (nextDistance < distance) {
                closest = i
                distance = nextDistance
            }
        }
        return closest
    }

    function setZoom(value) {
        root.zoom = root.zoomLevels[root.zoomLevelIndex(value)]
    }

    function zoomIn() {
        var index = root.zoomLevelIndex(root.zoom)
        if (index < root.zoomLevels.length - 1)
            root.zoom = root.zoomLevels[index + 1]
    }

    function zoomOut() {
        var index = root.zoomLevelIndex(root.zoom)
        if (index > 0)
            root.zoom = root.zoomLevels[index - 1]
    }

    function resetZoom() {
        root.setZoom(1.0)
    }

    function handleWheelZoom(delta, target) {
        if (!target)
            return false
        if (delta > 0)
            root.zoomIn()
        else if (delta < 0)
            root.zoomOut()
        return true
    }

    property alias listView: listView
    property alias gridView: gridView
    readonly property bool gridMode: backend && backend.view_mode === "grid"
    readonly property bool columnMode: backend && backend.view_mode === "column"
    readonly property int hoverAnimationDuration: Qt.styleHints.useHoverEffects ? 105 : 0
    property var wheelScrollView: null
    property real wheelTargetY: 0

    property int sizeWidth: 88
    property int modifiedWidth: 150
    property int kindWidth: 134

    // Selection is kept as path -> true so it survives sorting and reloads.
    property var selection: ({})
    property int anchorIndex: -1
    property int selectionCount: 0
    property string selectionText: ""

    // Per-tab selection, current row and scroll position.
    property var tabState: ({})
    property int boundTabId: -1
    property string lastPath: ""
    property string lastViewMode: backend ? backend.view_mode : ""
    property string pendingStartPath: ""
    property var pendingScrollRestore: null

    property var cutMap: ({})
    property var dragSession: null
    readonly property var dragPaths: dragSession ? dragSession.paths : []
    property string dragHoverPath: ""
    readonly property bool dragActive: dragSession !== null && dragSession.valid
    readonly property bool dragCopy: dragSession !== null && dragSession.copy
    readonly property bool dragAutoMove: dragSession !== null && dragSession.autoMove
    property string dragImageSource: ""
    property point dragPointer: Qt.point(-1, -1)
    property string hoverNavigationPath: ""
    property int dragHoverDelay: backend
                                  ? Math.max(200, Math.min(5000, backend.drag_hover_delay_ms))
                                  : 700
    property bool selectingByMouse: false
    property string selectionView: ""
    property point selectionOrigin: Qt.point(0, 0)
    property real selectionOriginContentY: 0
    property point selectionCurrent: Qt.point(0, 0)
    property point selectionPointer: Qt.point(0, 0)
    property var selectionBase: ({})

    signal requestTrash()
    signal requestPermanentDelete()
    signal requestNewFolder()
    signal requestNewFile()
    signal requestBatchRename()
    signal requestEmptyTrash()
    signal requestBookmark()
    signal requestOpenWith(string path, var paths)
    signal requestExtractTo(string path)
    signal requestTag(var paths)
    signal requestCompress(var paths)
    signal requestSearch(string query)

    Timer {
        id: mouseSelectionUpdate
        interval: 16
        repeat: false
        onTriggered: root.updateMouseSelection()
    }

    Timer {
        id: hoverNavigationTimer
        interval: root.dragHoverDelay
        repeat: false
        onTriggered: {
            var path = root.hoverNavigationPath
            root.hoverNavigationPath = ""
            if (path !== "" && root.backend && path !== root.backend.current_path)
                root.backend.navigate(path)
        }
    }

    Timer {
        id: mouseSelectionAutoScroll
        interval: 20
        repeat: true
        running: root.selectingByMouse
        onTriggered: {
            var view = root.activeView()
            var edge = Math.min(36, view.height / 3)
            var distance = root.selectionPointer.y < edge
                          ? root.selectionPointer.y - edge
                          : root.selectionPointer.y > view.height - edge
                            ? root.selectionPointer.y - (view.height - edge)
                            : 0
            if (distance === 0)
                return

            var delta = Math.sign(distance) * Math.min(28, Math.max(4, Math.abs(distance) * 0.65))
            var minY = view.originY
            var maxY = Math.max(minY, minY + view.contentHeight - view.height)
            var nextY = Math.max(minY, Math.min(maxY, view.contentY + delta))
            if (nextY === view.contentY)
                return
            view.contentY = nextY
            root.scheduleMouseSelectionUpdate()
        }
    }

    function activeView() {
        return root.gridMode ? gridView : root.columnMode ? columnListView : listView
    }

    function scrollByWheel(view, delta) {
        if (root.wheelScrollView !== view) {
            root.wheelScrollView = view
            root.wheelTargetY = view.contentY
        }
        var minY = view.originY
        var maxY = Math.max(minY, minY + view.contentHeight - view.height)
        root.wheelTargetY = Math.max(minY, Math.min(maxY, root.wheelTargetY - delta * 10))
        wheelAnimation.target = view
        wheelAnimation.from = view.contentY
        wheelAnimation.to = root.wheelTargetY
        wheelAnimation.restart()
    }

    function finishPendingScrollPositioning() {
        if (!root.backend || root.backend.loading)
            return
        Qt.callLater(function() {
            if (!root.backend || root.backend.loading)
                return
            var view = root.activeView()
            if (root.pendingScrollRestore) {
                var restore = root.pendingScrollRestore
                if (restore.tabId !== root.backend.active_tab_id)
                    return
                root.pendingScrollRestore = null
                if (restore.hasPosition) {
                    var minY = view.originY
                    var maxY = Math.max(minY, minY + view.contentHeight - view.height)
                    view.contentY = Math.max(minY, Math.min(maxY, minY + restore.y))
                } else {
                    view.positionViewAtBeginning()
                }
            } else if (root.pendingStartPath !== ""
                       && root.pendingStartPath === root.backend.current_path
                       && root.backend.active_tab_id === root.boundTabId) {
                root.pendingStartPath = ""
                view.positionViewAtBeginning()
                root.persistState()
            }
        })
    }

    NumberAnimation {
        id: wheelAnimation
        property: "contentY"
        duration: 75
        easing.type: Easing.OutQuad
    }

    readonly property string currentName: {
        var view = activeView()
        return view && view.currentIndex >= 0 && backend ? backend.name_at(view.currentIndex) : ""
    }
    readonly property string currentPath: {
        var view = activeView()
        return view && view.currentIndex >= 0 && backend ? backend.path_at(view.currentIndex) : ""
    }

    function nameWidth(total) {
        var used = 0
        if (!backend)
            return Math.max(120, total - 24)
        if (backend.show_size)
            used += sizeWidth + 8
        if (backend.show_modified)
            used += modifiedWidth + 8
        if (backend.show_kind)
            used += kindWidth + 8
        return Math.max(120, total - 24 - used)
    }

    function sortBy(key) {
        if (!backend)
            return
        var ascending = backend.sort_key === key ? !backend.sort_ascending : true
        backend.set_sort(key, ascending)
        listView.forceActiveFocus()
    }

    function selectedPaths() {
        var paths = []
        for (var path in selection) {
            if (selection[path])
                paths.push(path)
        }
        return paths
    }

    function beginDrag(paths, modifiers, imageSource) {
        if (!paths || paths.length === 0)
            return
        var sourcePaths = paths.slice()
        root.dragSession = {
            paths: sourcePaths,
            selection: sourcePaths.slice(),
            itemCount: sourcePaths.length,
            copy: (modifiers & Qt.ControlModifier) !== 0,
            autoMove: !(modifiers & (Qt.ControlModifier | Qt.ShiftModifier)),
            sourceDirectory: root.backend ? root.backend.current_path : "",
            destination: "",
            consumed: false,
            valid: true
        }
        root.dragImageSource = imageSource || ""
        root.dragHoverPath = ""
    }

    function resolveMainDropTarget(x, y) {
        if (!root.backend)
            return ""
        var view = root.activeView()
        var point = root.mapToItem(view, x, y)
        if (point.x < 0 || point.y < 0 || point.x >= view.width || point.y >= view.height)
            return ""
        var contentYPos = point.y + view.contentY
        var index = view.indexAt(point.x, contentYPos)
        if (index < 0 && contentYPos >= view.originY) {
            if (root.gridMode) {
                var col = Math.floor(point.x / root.gridCellWidth)
                if (col >= 0 && col < root.gridColumnCount) {
                    var r = Math.floor((contentYPos - view.originY) / root.gridCellHeight)
                    index = r * root.gridColumnCount + col
                }
            } else {
                index = Math.floor((contentYPos - view.originY) / root.effectiveRowHeight)
            }
        }
        if (index >= 0 && index < root.backend.item_count && root.backend.is_dir_at(index)) {
            var targetPath = root.backend.path_at(index)
            if (root.dragPaths.indexOf(targetPath) < 0)
                return targetPath
        }
        return root.backend.current_path
    }

    function resolveDropTarget(x, y) {
        if (root.externalDropResolver) {
            var windowPoint = root.mapToItem(null, x, y)
            var external = root.externalDropResolver(windowPoint)
            if (external)
                return external
        }
        return root.resolveMainDropTarget(x, y)
    }

    function updateDragPointer(x, y) {
        root.dragPointer = Qt.point(x, y)
        if (!root.dragActive)
            return
        var destination = root.resolveDropTarget(x, y)
        if (destination !== "" && destination !== root.backend.current_path) {
            if (root.dragHoverPath !== destination) {
                root.setDragHoverPath(destination)
                root.beginFolderHover(destination)
            }
        } else {
            var previous = root.hoverNavigationPath
            if (previous !== "")
                root.endFolderHover(previous)
            root.setDragHoverPath("")
        }
    }

    function finishInternalDrag(x, y) {
        if (!root.dragActive)
            return
        var destination = root.resolveDropTarget(x, y)
        if (destination !== "")
            root.dropDraggedPaths(destination, root.dragCopy ? Qt.CopyAction : Qt.MoveAction)
        root.finishDrag()
    }

    function beginFolderHover(path) {
        if (!path || path === "" || !root.backend)
            return
        root.hoverNavigationPath = path
        hoverNavigationTimer.restart()
    }

    function setDragHoverPath(path) {
        var destination = path || ""
        root.dragHoverPath = destination
        if (root.dragSession && !root.dragSession.consumed) {
            var session = root.dragSession
            root.dragSession = {
                paths: session.paths,
                selection: session.selection,
                itemCount: session.itemCount,
                copy: session.copy,
                autoMove: session.autoMove,
                sourceDirectory: session.sourceDirectory,
                destination: destination,
                consumed: false,
                valid: session.valid
            }
        }
    }

    function endFolderHover(path) {
        if (root.hoverNavigationPath === path) {
            hoverNavigationTimer.stop()
            root.hoverNavigationPath = ""
        }
    }

    function finishDrag() {
        var session = root.dragSession
        var clearDraggedSelection = session && root.backend
                && (session.consumed || root.backend.current_path !== session.sourceDirectory)
        hoverNavigationTimer.stop()
        root.hoverNavigationPath = ""
        root.dragSession = null
        root.dragHoverPath = ""
        root.dragImageSource = ""
        root.dragPointer = Qt.point(-1, -1)
        root.resolveDropTarget(-1000, -1000)
        if (clearDraggedSelection)
            root.clearSelection()
    }

    function dropDraggedPaths(dest, proposedAction) {
        var session = root.dragSession
        if (!backend || !dest || dest === "" || !session || !session.valid
                || session.paths.length === 0 || session.consumed)
            return false
        var moveItems = proposedAction === Qt.CopyAction
                ? false : proposedAction === Qt.MoveAction ? true : !session.copy
        var autoMove = session.autoMove && moveItems && proposedAction !== Qt.CopyAction
        root.dragSession = {
            paths: session.paths,
            selection: session.selection,
            itemCount: session.itemCount,
            copy: !moveItems,
            autoMove: autoMove,
            sourceDirectory: session.sourceDirectory,
            destination: dest,
            consumed: true,
            valid: session.valid
        }
        backend.drop_paths(dest, session.paths.slice(), moveItems, autoMove, "ask")
        return true
    }

    function dropExternalPaths(drop, dest) {
        if (!backend || !dest || dest === "" || !drop || !drop.urls || drop.urls.length === 0)
            return false
        var urls = []
        for (var i = 0; i < drop.urls.length; i++)
            urls.push(drop.urls[i].toString())
        backend.drop_external_paths(dest, urls, drop.proposedAction === Qt.MoveAction)
        return true
    }

    function isSelected(path) {
        return selection[path] === true
    }

    function isCut(path) {
        return backend && backend.clipboard_cut && cutMap[path] === true
    }

    function updateStats() {
        selectionCount = selectedPaths().length
        selectionText = backend && selectionCount > 0
                        ? backend.selection_info(selectedPaths()).text
                        : ""
    }

    function persistState() {
        if (boundTabId < 0 || !backend)
            return
        var next = {}
        for (var key in tabState)
            next[key] = tabState[key]
        next[boundTabId] = {
            paths: selectedPaths(),
            index: activeView().currentIndex,
            y: activeView().contentY - activeView().originY
        }
        tabState = next
    }

    function commitSelection(map) {
        selection = map
        updateStats()
        persistState()
    }

    function clearSelection() {
        anchorIndex = -1
        commitSelection({})
    }

    function setSingleSelection(path, index) {
        anchorIndex = index
        var map = {}
        if (path !== "")
            map[path] = true
        commitSelection(map)
    }

    function toggleSelection(path, index) {
        var map = {}
        for (var key in selection)
            map[key] = selection[key]
        if (map[path])
            delete map[path]
        else
            map[path] = true
        anchorIndex = index
        commitSelection(map)
    }

    function extendSelection(index) {
        if (anchorIndex < 0)
            anchorIndex = listView.currentIndex < 0 ? index : listView.currentIndex
        var from = Math.min(anchorIndex, index)
        var to = Math.max(anchorIndex, index)
        var map = {}
        for (var i = from; i <= to; i++) {
            var path = backend.path_at(i)
            if (path !== "")
                map[path] = true
        }
        commitSelection(map)
    }

    function selectAll() {
        if (!backend)
            return
        var map = {}
        for (var i = 0; i < backend.item_count; i++) {
            var path = backend.path_at(i)
            if (path !== "")
                map[path] = true
        }
        anchorIndex = 0
        commitSelection(map)
    }

    function beginMouseSelection(viewName, x, y, modifiers) {
        selectingByMouse = true
        selectionView = viewName
        selectionOrigin = Qt.point(x, y)
        selectionCurrent = selectionOrigin
        selectionOriginContentY = activeView().contentY + y
        selectionBase = {}
        if (modifiers & Qt.ControlModifier) {
            for (var path in selection) {
                if (selection[path])
                    selectionBase[path] = true
            }
        }
        scheduleMouseSelectionUpdate()
    }

    function scheduleMouseSelectionUpdate() {
        if (!mouseSelectionUpdate.running)
            mouseSelectionUpdate.start()
    }

    function updateMouseSelectionPointer(x, y) {
        var view = activeView()
        selectionPointer = Qt.point(x, y)
        selectionCurrent = Qt.point(
            Math.max(0, Math.min(view.width, x)),
            Math.max(0, Math.min(view.height, y))
        )
        scheduleMouseSelectionUpdate()
    }

    function updateMouseSelection() {
        if (!selectingByMouse || !backend)
            return
        var left = Math.min(selectionOrigin.x, selectionCurrent.x)
        var right = Math.max(selectionOrigin.x, selectionCurrent.x)
        var map = {}
        for (var existing in selectionBase)
            map[existing] = true

        if (selectionView === "list" || selectionView === "column") {
            var list = activeView()
            var contentTop = Math.min(selectionOriginContentY,
                                      selectionCurrent.y + list.contentY)
            var contentBottom = Math.max(selectionOriginContentY,
                                         selectionCurrent.y + list.contentY)
            var first = Math.max(0, Math.floor((contentTop - list.originY) / effectiveRowHeight))
            var last = Math.min(backend.item_count - 1,
                                Math.floor((contentBottom - list.originY) / effectiveRowHeight))
            var listPaths = backend.paths_in_range(first, last)
            for (var offset = 0; offset < listPaths.length; offset++) {
                var i = first + offset
                var rowTop = list.originY + i * effectiveRowHeight
                if (rowTop + effectiveRowHeight >= contentTop && rowTop <= contentBottom) {
                    var listPath = listPaths[offset]
                    if (listPath !== "")
                        map[listPath] = true
                }
            }
        } else if (selectionView === "grid") {
            var columns = root.gridColumnCount
            var contentTop = Math.min(selectionOriginContentY,
                                      selectionCurrent.y + gridView.contentY)
            var contentBottom = Math.max(selectionOriginContentY,
                                         selectionCurrent.y + gridView.contentY)
            var firstRow = Math.max(0, Math.floor((contentTop - gridView.originY)
                                                 / gridView.cellHeight))
            var lastRow = Math.min(
                Math.ceil(backend.item_count / columns) - 1,
                Math.floor((contentBottom - gridView.originY) / gridView.cellHeight)
            )
            var firstColumn = Math.max(0, Math.floor(left / gridView.cellWidth))
            var lastColumn = Math.min(columns - 1, Math.floor(right / gridView.cellWidth))
            var firstIndex = firstRow * columns + firstColumn
            var lastIndex = lastRow * columns + lastColumn
            var gridPaths = backend.paths_in_range(firstIndex, lastIndex)
            for (var offset = 0; offset < gridPaths.length; offset++) {
                var index = firstIndex + offset
                var column = index % columns
                if (column < firstColumn || column > lastColumn)
                    continue
                var gridPath = gridPaths[offset]
                if (gridPath !== "")
                    map[gridPath] = true
            }
        }
        selection = map
    }

    function finishMouseSelection() {
        if (!selectingByMouse)
            return
        mouseSelectionUpdate.stop()
        updateMouseSelection()
        selectingByMouse = false
        selectionView = ""
        selectionBase = ({})
        selectionPointer = Qt.point(0, 0)
        updateStats()
        persistState()
    }

    function pruneSelection() {
        var current = selectedPaths()
        if (current.length === 0) {
            updateStats()
            return
        }
        var present = backend.filter_present(current)
        if (present.length === current.length) {
            updateStats()
            return
        }
        var map = {}
        for (var i = 0; i < present.length; i++)
            map[present[i]] = true
        commitSelection(map)
    }

    function moveBy(delta, extend, keepSelection) {
        if (!backend || backend.item_count === 0)
            return
        var view = activeView()
        var from = view.currentIndex < 0 ? 0 : view.currentIndex
        var to = Math.max(0, Math.min(backend.item_count - 1, from + delta))
        moveToIndex(view, to, extend, keepSelection)
    }

    function moveGrid(dx, dy, extend, keepSelection) {
        if (!backend || backend.item_count === 0)
            return
        var from = gridView.currentIndex < 0 ? 0 : gridView.currentIndex
        var columns = root.gridColumnCount
        var row = Math.floor(from / columns)
        var column = from % columns
        if (dx !== 0) {
            if ((dx < 0 && column === 0) || (dx > 0 && column >= columns - 1))
                return
            column += dx
        }
        if (dy !== 0)
            row += dy

        var target = row * columns + column
        if (target < 0 || target >= backend.item_count)
            return
        moveToIndex(gridView, target, extend, keepSelection)
    }

    function moveToIndex(view, to, extend, keepSelection) {
        if (to === view.currentIndex && view.currentIndex >= 0)
            return
        view.currentIndex = to
        if (extend) {
            extendSelection(to)
        } else if (!keepSelection) {
            setSingleSelection(backend.path_at(to), to)
        }
        view.positionViewAtIndex(to, ListView.Contain)
        view.forceActiveFocus()
    }

    function pageSize() {
        var view = activeView()
        var row = theme ? theme.row_height : 34
        return Math.max(1, Math.floor(view.height / Math.max(1, row)) - 1)
    }

    function restoreTab(id) {
        lastPath = backend ? backend.current_path : ""
        var state = tabState[id]
        selection = {}
        anchorIndex = -1
        var view = activeView()
        if (state) {
            var present = backend.filter_present(state.paths || [])
            var map = {}
            for (var i = 0; i < present.length; i++)
                map[present[i]] = true
            selection = map
            var count = backend.item_count
            if (count > 0) {
                view.currentIndex = Math.max(0, Math.min(state.index || 0, count - 1))
            } else {
                view.currentIndex = -1
            }
        } else {
            view.currentIndex = backend.item_count > 0 ? 0 : -1
        }
        pendingStartPath = ""
        pendingScrollRestore = {
            tabId: id,
            hasPosition: !!state,
            y: state ? state.y || 0 : 0
        }
        finishPendingScrollPositioning()
        updateStats()
    }

    function showInFolder(path) {
        var slash = path.lastIndexOf("/")
        if (slash > 0 && backend)
            backend.open_path(path.substring(0, slash))
    }

    function openItemMenu(index, point) {
        var path = backend.path_at(index)
        if (path !== "" && !isSelected(path))
            setSingleSelection(path, index)
        menu.entries = itemMenuEntries(index)
        menu.open(point.x, point.y)
    }

    function openBackgroundMenu(x, y) {
        menu.entries = backgroundMenuEntries()
        menu.open(x, y)
    }

    function itemMenuEntries(index) {
        var selected = selectedPaths()
        var many = selected.length > 1
        var path = backend.path_at(index)
        var isDir = backend.is_dir_at(index)
        var inTrash = backend.in_trash
        var entries = []
        var defaultApp = !isDir && !many && !inTrash
                         ? backend.default_app_name(path) : ""

        entries.push({
            label: isDir ? "Open Folder"
                         : (defaultApp !== "" ? "Open with " + defaultApp : "Open"),
            shortcut: "Enter",
            action: function() { backend.open_path(path) }
        })
        if (!isDir && !many && !inTrash) {
            entries.push({
                label: "Open With\u2026",
                action: function() { root.requestOpenWith(path, selected.slice()) }
            })
        }
        if (isDir) {
            entries.push({
                label: "Open in New Tab",
                action: function() { backend.open_in_new_tab(index) }
            })
        }
        if (backend.search_mode || backend.virtual_collection) {
            entries.push({
                label: "Show in Folder",
                action: function() { root.showInFolder(path) }
            })
        }
        entries.push({ separator: true })

        if (inTrash) {
            entries.push({
                label: "Restore",
                action: function() { backend.restore_paths(selected) }
            })
            entries.push({ separator: true })
        }

        entries.push({
            label: many ? "Copy " + selected.length + " Items" : "Copy",
            shortcut: "Ctrl+C",
            action: function() { backend.copy_paths(selected) }
        })
        entries.push({
            label: many ? "Cut " + selected.length + " Items" : "Cut",
            shortcut: "Ctrl+X",
            action: function() { backend.cut_paths(selected) }
        })
        entries.push({
            label: "Paste Here",
            shortcut: "Ctrl+V",
            enabled: backend.clipboard_count > 0 && !backend.virtual_collection,
            action: function() { backend.paste(backend.current_path, "ask") }
        })
        entries.push({ separator: true })

        if (!many) {
            entries.push({
                label: "Rename\u2026",
                shortcut: "F2",
                action: function() { backend.begin_rename(path) }
            })
        }
        if (selected.length > 1 && !inTrash) {
            entries.push({
                label: "Batch Rename\u2026",
                action: function() { root.requestBatchRename() }
            })
        }
        if (!inTrash) {
            entries.push({
                label: selected.length > 1
                       ? "Properties for " + selected.length + " Items"
                       : "Properties",
                action: function() { backend.show_properties_for_paths(selected.slice()) }
            })
        }
        if (!inTrash) {
            entries.push({
                label: "Assign Tag\u2026",
                action: function() { root.requestTag(selected.slice()) }
            })
        }
        if (!inTrash) {
            if (!backend.virtual_collection) {
                entries.push({
                    label: "Compress\u2026",
                    enabled: selected.length > 0,
                    action: function() { root.requestCompress(selected.slice()) }
                })
            }
            if (!many && backend.is_archive(path)) {
                if (!backend.virtual_collection) {
                    entries.push({
                        label: "Extract Here",
                        action: function() { backend.extract_path(path, backend.current_path) }
                    })
                }
                entries.push({
                    label: "Extract To\u2026",
                    action: function() { root.requestExtractTo(path) }
                })
            }
        }
        if (!inTrash) {
            entries.push({
                label: "Move to Trash",
                shortcut: "Delete",
                action: function() { root.requestTrash() }
            })
        }
        entries.push({
            label: "Delete Permanently\u2026",
            shortcut: "Shift+Delete",
            danger: true,
            action: function() { root.requestPermanentDelete() }
        })
        entries.push({ separator: true })
        entries.push({
            label: "Copy Path",
            action: function() { backend.copy_paths_text(selected) }
        })
        if (isDir && !many) {
            entries.push({
                label: "Bookmark This Folder",
                action: function() { backend.add_bookmark(path, "") }
            })
        }
        return entries
    }

    function backgroundMenuEntries() {
        var entries = []
        entries.push({
            label: "New Folder\u2026",
            shortcut: "Ctrl+Shift+N",
            enabled: !backend.virtual_collection,
            action: function() { root.requestNewFolder() }
        })
        entries.push({
            label: "New File\u2026",
            enabled: !backend.virtual_collection,
            action: function() { root.requestNewFile() }
        })
        entries.push({ separator: true })
        entries.push({
            label: "Paste",
            shortcut: "Ctrl+V",
            enabled: backend.clipboard_count > 0 && !backend.virtual_collection,
            action: function() { backend.paste(backend.current_path, "ask") }
        })
        entries.push({
            label: "Select All",
            shortcut: "Ctrl+A",
            enabled: backend.item_count > 0,
            action: function() { root.selectAll() }
        })
        entries.push({ separator: true })
        if (!backend.virtual_collection) entries.push({
            label: "Bookmark This Folder",
            action: function() { root.requestBookmark() }
        })
        if (!backend.virtual_collection) entries.push({
            label: "Open Terminal Here",
            action: function() { backend.open_terminal(backend.current_path) }
        })
        if (backend.item_count > 1 && !backend.in_trash) {
            entries.push({
                label: "Batch Rename Selected\u2026",
                action: function() { root.requestBatchRename() }
            })
        }
        entries.push({ separator: true })
        entries.push({
            label: backend.show_hidden ? "Hide Hidden Files" : "Show Hidden Files",
            shortcut: "Ctrl+H",
            action: function() { backend.toggle_hidden() }
        })
        entries.push({
            label: "Refresh",
            shortcut: "F5",
            action: function() { backend.refresh() }
        })
        if (backend.in_trash) {
            entries.push({ separator: true })
            entries.push({
                label: "Empty Trash\u2026",
                danger: true,
                action: function() { root.requestEmptyTrash() }
            })
        }
        if (backend.recent_view) {
            entries.push({ separator: true })
            entries.push({
                label: "Clear Recent History",
                action: function() { backend.clear_recent_history() }
            })
        }
        return entries
    }

    Column {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            width: parent.width
            height: 30
            color: theme ? theme.surface : "#151820"

            Row {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                spacing: 8

                HeaderCell {
                    width: root.nameWidth(root.width)
                    height: parent.height
                    theme: root.theme
                    label: "Name"
                    active: backend ? backend.sort_key === "name" : false
                    ascending: backend ? backend.sort_ascending : true
                    onClicked: root.sortBy("name")
                }

                HeaderCell {
                    visible: backend ? backend.show_size : true
                    width: visible ? root.sizeWidth : 0
                    height: parent.height
                    theme: root.theme
                    label: "Size"
                    alignment: Text.AlignRight
                    active: backend ? backend.sort_key === "size" : false
                    ascending: backend ? backend.sort_ascending : true
                    onClicked: root.sortBy("size")
                }

                HeaderCell {
                    visible: backend ? backend.show_modified : true
                    width: visible ? root.modifiedWidth : 0
                    height: parent.height
                    theme: root.theme
                    label: backend && backend.recent_view ? "Date Opened" : "Modified"
                    active: backend ? backend.sort_key === "modified" : false
                    ascending: backend ? backend.sort_ascending : true
                    onClicked: root.sortBy("modified")
                }

                HeaderCell {
                    visible: backend ? backend.show_kind : true
                    width: visible ? root.kindWidth : 0
                    height: parent.height
                    theme: root.theme
                    label: backend && (backend.search_mode || backend.recent_view || backend.tag_collection_view)
                           ? (backend.recent_view ? "Type / Location" : "Location") : "Kind"
                    active: backend ? backend.sort_key === "kind" : false
                    ascending: backend ? backend.sort_ascending : true
                    onClicked: root.sortBy("kind")
                }
            }
        }

        Rectangle {
            width: parent.width
            height: 1
            color: theme ? theme.border : "#292e3a"
        }

        Item {
            id: listContainer
            width: parent.width
            height: parent.height - 31

            Rectangle {
                anchors.fill: parent
                color: theme ? theme.background : "#0f1115"
            }

            ListView {
                id: listView
                anchors.fill: parent
                clip: true
                focus: true
                visible: !root.gridMode && !root.columnMode
                keyNavigationEnabled: false
                model: root.backend
                spacing: 0
                boundsBehavior: Flickable.StopAtBounds
                maximumFlickVelocity: 12000
                flickDeceleration: 9000
                cacheBuffer: 400

                delegate: FileRow {
                    theme: root.theme
                    backend: root.backend
                    area: root
                }

                DropArea {
                    anchors.fill: parent
                    z: -1
                    keys: ["text/uri-list", "filem/paths"]
                    onEntered: {
                        if (root.dragActive && root.backend)
                            root.setDragHoverPath(root.backend.current_path)
                    }
                    onExited: {
                        if (root.dragActive
                                && root.backend && root.dragHoverPath === root.backend.current_path)
                            root.setDragHoverPath("")
                    }
                    onDropped: function(drop) {
                        if (root.dragActive) {
                            if (root.dropDraggedPaths(root.backend.current_path, drop.proposedAction))
                                drop.acceptProposedAction()
                        } else if (root.dropExternalPaths(drop, root.backend.current_path)) {
                            drop.acceptProposedAction()
                        }
                    }
                }

                WheelHandler {
                    acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                    onWheel: function(event) {
                        if (event.modifiers & Qt.ControlModifier) {
                            event.accepted = true
                            if (event.angleDelta.y > 0)
                                root.zoomIn()
                            else if (event.angleDelta.y < 0)
                                root.zoomOut()
                        } else {
                            var delta = event.pixelDelta.y !== 0
                                        ? event.pixelDelta.y
                                        : (event.angleDelta.y / 120) * 48
                            root.scrollByWheel(listView, delta)
                            event.accepted = true
                        }
                    }
                }

                Keys.onPressed: function(event) {
                    if (!root.backend)
                        return
                    if (!(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))
                            && /^[^\u0000-\u001f\u007f]+$/.test(event.text)) {
                        root.requestSearch(event.text)
                        event.accepted = true
                        return
                    }
                    var extend = (event.modifiers & Qt.ShiftModifier) !== 0
                    var keep = (event.modifiers & Qt.ControlModifier) !== 0

                    switch (event.key) {
                    case Qt.Key_Return:
                    case Qt.Key_Enter:
                        if (listView.currentIndex >= 0) {
                            if (keep)
                                root.backend.open_in_new_tab(listView.currentIndex)
                            else
                                root.backend.open_index(listView.currentIndex)
                        }
                        event.accepted = true
                        break
                    case Qt.Key_Backspace:
                        root.backend.go_up()
                        event.accepted = true
                        break
                    case Qt.Key_Up:
                        root.moveBy(-1, extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_Down:
                        root.moveBy(1, extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_Right:
                        if (listView.currentIndex >= 0
                                && root.backend.tree_expandable_at(listView.currentIndex)) {
                            if (!root.backend.tree_expanded_at(listView.currentIndex))
                                root.backend.set_tree_expanded(listView.currentIndex, true)
                            else if (listView.currentIndex + 1 < root.backend.item_count
                                     && root.backend.tree_depth_at(listView.currentIndex + 1)
                                        > root.backend.tree_depth_at(listView.currentIndex))
                                root.moveBy(1, extend, keep)
                            event.accepted = true
                        }
                        break
                    case Qt.Key_Left:
                        if (listView.currentIndex >= 0
                                && root.backend.tree_expanded_at(listView.currentIndex)) {
                            root.backend.set_tree_expanded(listView.currentIndex, false)
                            event.accepted = true
                        } else if (listView.currentIndex >= 0) {
                            var parentIndex = root.backend.tree_parent_index(listView.currentIndex)
                            if (parentIndex >= 0) {
                                listView.currentIndex = parentIndex
                                root.setSingleSelection(root.backend.path_at(parentIndex), parentIndex)
                                event.accepted = true
                            }
                        }
                        break
                    case Qt.Key_PageUp:
                        root.moveBy(-root.pageSize(), extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_PageDown:
                        root.moveBy(root.pageSize(), extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_Home:
                        root.moveBy(-root.backend.item_count, extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_End:
                        root.moveBy(root.backend.item_count, extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_Escape:
                        if (root.backend.op_active && root.backend.op_cancelable) {
                            root.backend.cancel_operation()
                            event.accepted = true
                        } else if (root.selectionCount > 0) {
                            root.clearSelection()
                            event.accepted = true
                        }
                        break
                    }
                }

                TapHandler {
                    acceptedButtons: Qt.LeftButton
                    onTapped: function(point) {
                        if (listView.indexAt(point.position.x,
                                             point.position.y + listView.contentY) < 0) {
                            root.clearSelection()
                            listView.currentIndex = -1
                            listView.forceActiveFocus()
                        }
                    }
                }

                TapHandler {
                    acceptedButtons: Qt.RightButton
                    onTapped: function(point) {
                        root.openBackgroundMenu(point.position.x, point.position.y)
                    }
                }
            }

            Column {
                id: columnView
                anchors.fill: parent
                visible: root.columnMode
                spacing: 0

                ListView {
                    id: columnListView
                    width: parent.width
                    height: parent.height
                    focus: true
                    keyNavigationEnabled: false
                    model: root.backend
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds
                    maximumFlickVelocity: 12000
                    flickDeceleration: 9000
                    delegate: FileRow {
                        theme: root.theme
                        backend: root.backend
                        area: root
                    }

                    DropArea {
                        anchors.fill: parent
                        z: -1
                        keys: ["text/uri-list", "filem/paths"]
                        onEntered: {
                            if (root.backend && root.dragActive)
                                root.setDragHoverPath(root.backend.current_path)
                        }
                        onExited: {
                            if (root.backend
                                    && root.dragHoverPath === root.backend.current_path)
                                root.setDragHoverPath("")
                        }
                        onDropped: function(drop) {
                            if (!root.backend)
                                return
                            if (root.dragActive) {
                                if (root.dropDraggedPaths(
                                            root.backend.current_path, drop.proposedAction))
                                    drop.acceptProposedAction()
                            } else if (root.dropExternalPaths(drop, root.backend.current_path)) {
                                drop.acceptProposedAction()
                            }
                        }
                    }

                    Keys.onPressed: function(event) {
                        if (!root.backend)
                            return
                        var extend = (event.modifiers & Qt.ShiftModifier) !== 0
                        var keep = (event.modifiers & Qt.ControlModifier) !== 0
                        switch (event.key) {
                        case Qt.Key_Up:
                            root.moveBy(-1, extend, keep)
                            event.accepted = true
                            break
                        case Qt.Key_Down:
                            root.moveBy(1, extend, keep)
                            event.accepted = true
                            break
                        case Qt.Key_Left:
                        case Qt.Key_Backspace:
                            root.backend.go_up()
                            event.accepted = true
                            break
                        case Qt.Key_Right:
                            if (columnListView.currentIndex >= 0
                                    && root.backend.is_dir_at(columnListView.currentIndex))
                                root.backend.open_index(columnListView.currentIndex)
                            event.accepted = true
                            break
                        case Qt.Key_Return:
                        case Qt.Key_Enter:
                            if (columnListView.currentIndex >= 0) {
                                if (keep)
                                    root.backend.open_in_new_tab(columnListView.currentIndex)
                                else
                                    root.backend.open_index(columnListView.currentIndex)
                            }
                            event.accepted = true
                            break
                        case Qt.Key_PageUp:
                            root.moveBy(-root.pageSize(), extend, keep)
                            event.accepted = true
                            break
                        case Qt.Key_PageDown:
                            root.moveBy(root.pageSize(), extend, keep)
                            event.accepted = true
                            break
                        case Qt.Key_Home:
                            root.moveBy(-root.backend.item_count, extend, keep)
                            event.accepted = true
                            break
                        case Qt.Key_End:
                            root.moveBy(root.backend.item_count, extend, keep)
                            event.accepted = true
                            break
                        case Qt.Key_Escape:
                            if (root.selectionCount > 0) {
                                root.clearSelection()
                                event.accepted = true
                            }
                            break
                        }
                    }

                    WheelHandler {
                        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                        onWheel: function(event) {
                            if (event.modifiers & Qt.ControlModifier) {
                                event.accepted = true
                                if (event.angleDelta.y > 0)
                                    root.zoomIn()
                                else if (event.angleDelta.y < 0)
                                    root.zoomOut()
                            } else {
                                var delta = event.pixelDelta.y !== 0
                                            ? event.pixelDelta.y
                                            : (event.angleDelta.y / 120) * 48
                                root.scrollByWheel(columnListView, delta)
                                event.accepted = true
                            }
                        }
                    }

                    TapHandler {
                        acceptedButtons: Qt.LeftButton
                        onTapped: function(point) {
                            if (columnListView.indexAt(point.position.x,
                                                       point.position.y + columnListView.contentY) < 0) {
                                root.clearSelection()
                                columnListView.currentIndex = -1
                                columnListView.forceActiveFocus()
                            }
                        }
                    }

                    TapHandler {
                        acceptedButtons: Qt.RightButton
                        onTapped: function(point) {
                            root.openBackgroundMenu(point.position.x, point.position.y)
                        }
                    }
                }
            }

            GridView {
                id: gridView
                anchors.fill: parent
                anchors.rightMargin: 12
                visible: root.gridMode
                clip: true
                focus: true
                model: root.backend
                cellWidth: root.gridCellWidth
                cellHeight: root.gridCellHeight
                boundsBehavior: Flickable.StopAtBounds
                maximumFlickVelocity: 12000
                flickDeceleration: 9000
                cacheBuffer: 200

                WheelHandler {
                    acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                    onWheel: function(event) {
                        if (event.modifiers & Qt.ControlModifier) {
                            event.accepted = true
                            if (event.angleDelta.y > 0)
                                root.zoomIn()
                            else if (event.angleDelta.y < 0)
                                root.zoomOut()
                            return
                        }

                        var delta = event.pixelDelta.y !== 0
                                    ? event.pixelDelta.y
                                    : (event.angleDelta.y / 120) * 48
                        root.scrollByWheel(gridView, delta)
                        event.accepted = true
                    }
                }

                delegate: Item {
                    id: gridDelegate
                    width: gridView.cellWidth
                    height: gridView.cellHeight
                    property bool gridSelected: root.isSelected(model.path)
                    property bool gridFocused: gridView.activeFocus
                                               && gridView.currentIndex === index
                    property bool gridDragged: root.dragActive
                                               && root.dragPaths.indexOf(model.path) >= 0
                    property bool dragTarget: model.isDir
                                              && ((root.dragActive
                                                   && root.dragHoverPath === model.path)
                                                  || gridDropArea.containsDrag)
                    property bool gridHovered: gridMouse.containsMouse && !gridSelected
                    property string entryPath: model.path
                    property bool imageThumbnail: [
                        "png", "jpg", "jpeg", "webp", "gif", "svg", "bmp", "tif", "tiff", "avif", "ico",
                        "heic", "heif", "jxl", "tga", "jp2", "j2k"
                    ].indexOf((model.extension || "").toLowerCase()) >= 0
                    property bool videoThumbnail: [
                        "mp4", "mkv", "webm", "avi", "mov", "m4v", "mpg", "mpeg", "ogv",
                        "flv", "wmv", "3gp", "3g2", "ts", "mts", "m2ts", "vob"
                    ].indexOf((model.extension || "").toLowerCase()) >= 0
                    property bool audioThumbnail: [
                        "mp3", "flac", "ogg", "oga", "opus", "m4a", "m4b", "wav", "wave",
                        "aac", "wma", "aif", "aiff", "ape", "wv", "3ga"
                    ].indexOf((model.extension || "").toLowerCase()) >= 0
                    property bool generatedThumbnail: (imageThumbnail
                                                       && (model.extension || "").toLowerCase() !== "svg")
                            || videoThumbnail || audioThumbnail
                            || (model.extension || "").toLowerCase() === "pdf"
                    property string thumbnailSource: model.iconUrl || ""
                    property string thumbnailVersion: (model.sizeText || "")
                                                       + "\u0000" + (model.modifiedMs || 0)
                    readonly property bool thumbnailInViewport:
                        y + height > gridView.contentY
                        && y < gridView.contentY + gridView.height

                    function refreshThumbnail() {
                        if (!root.backend)
                            return
                        if (imageThumbnail && (model.extension || "").toLowerCase() === "svg") {
                            thumbnailSource = root.backend.file_url(entryPath)
                        } else if (generatedThumbnail) {
                            var cached = root.backend.thumbnail_url(entryPath)
                            if (cached !== "") {
                                setThumbnailSource(cached)
                            } else {
                                setThumbnailSource(model.iconUrl || "")
                                if (thumbnailInViewport)
                                    root.backend.request_thumbnail(entryPath, true)
                            }
                        } else {
                            setThumbnailSource(model.iconUrl || "")
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
                        if (thumbnailInViewport && generatedThumbnail
                                && thumbnailSource === (model.iconUrl || ""))
                            refreshThumbnail()
                    }

                    Connections {
                        target: root.backend
                        function onThumbnail_ready_changed() {
                            if (gridDelegate.generatedThumbnail
                                    && root.backend.thumbnail_ready_path === gridDelegate.entryPath)
                                gridDelegate.refreshThumbnail()
                        }
                    }

                    Rectangle {
                        id: cardBackground
                        anchors.fill: parent
                        anchors.margins: 2
                        radius: theme ? Math.min(12, theme.radius) : 9
                        color: dragTarget
                               ? (theme ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, 0.2) : "#337ea7ff")
                               : (gridSelected
                                  ? (theme ? theme.selection : "#24364f")
                                  : "transparent")
                        border.width: gridSelected || dragTarget || gridFocused || gridDragged ? 1 : 0
                        border.color: gridDragged || gridSelected
                                      ? (theme ? theme.accent : "#8ab4f8")
                                      : (theme ? theme.focus : "#8ab4f8")
                    }

                    Rectangle {
                        id: hoverOverlay
                        anchors.fill: cardBackground
                        anchors.margins: 1
                        radius: Math.max(0, cardBackground.radius - 1)
                        color: theme ? theme.hover : "#202531"
                        opacity: gridDelegate.gridDragged || dragTarget
                                 || gridDelegate.gridSelected || !gridDelegate.gridHovered ? 0 : 0.78

                        Behavior on opacity {
                            enabled: !gridDelegate.gridSelected
                            NumberAnimation {
                                duration: root.hoverAnimationDuration
                                easing.type: Easing.OutCubic
                            }
                        }
                    }

                    Rectangle {
                        anchors.fill: cardBackground
                        anchors.margins: 1
                        radius: Math.max(0, cardBackground.radius - 1)
                        color: theme ? theme.accent : "#8ab4f8"
                        opacity: gridMouse.pressed && !gridDelegate.gridDragged && !dragTarget
                                 ? 0.18 : 0
                    }

                    Column {
                        anchors.fill: cardBackground
                        anchors.leftMargin: 7
                        anchors.rightMargin: 7
                        anchors.topMargin: 6
                        anchors.bottomMargin: 6
                        spacing: 1

                        Item {
                            width: parent.width
                            height: root.gridIconSize

                            Image {
                                anchors.centerIn: parent
                                source: gridDelegate.thumbnailSource || model.iconUrl || ""
                                width: root.gridIconSize
                                height: root.gridIconSize
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                sourceSize.width: width
                                sourceSize.height: height
                                asynchronous: true
                                cache: true
                            }
                        }

                        Text {
                            anchors.horizontalCenter: parent.horizontalCenter
                            width: Math.min(parent.width, root.gridIconSize + 28 * root.zoom)
                            height: root.gridLabelHeight
                            text: root.backend && root.backend.recent_view
                                  ? (model.name || "") + "\n"
                                    + (model.kind || "") + "\n"
                                    + (model.displayPath || "") + "\n"
                                    + (model.modifiedMs > 0
                                       ? Qt.formatDateTime(new Date(model.modifiedMs), "d MMM yyyy  HH:mm")
                                       : "\u2014")
                                    + "  \u00b7  " + (model.sizeText || "")
                                  : (model.name || "")
                            color: theme ? theme.foreground : "#e8edf7"
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: (root.backend && root.backend.recent_view
                                            ? Math.max(9, root.gridLabelFontSize * 0.82)
                                            : root.gridLabelFontSize) * (theme ? theme.font_scale : 1)
                            font.weight: gridSelected ? Font.Medium : Font.Normal
                            verticalAlignment: Text.AlignVCenter
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            maximumLineCount: root.backend && root.backend.recent_view ? 5 : 3
                            elide: Text.ElideRight
                            horizontalAlignment: Text.AlignHCenter
                            clip: true
                            Accessible.name: model.name || ""
                        }

                    }

                    DropArea {
                        id: gridDropArea
                        anchors.fill: parent
                        z: 3
                        keys: ["text/uri-list", "filem/paths"]
                        onEntered: {
                            if (model.isDir) {
                                if (root.dragActive)
                                    root.setDragHoverPath(model.path)
                                root.beginFolderHover(model.path)
                            }
                        }
                        onExited: {
                            if (root.dragHoverPath === model.path)
                                root.setDragHoverPath("")
                            root.endFolderHover(model.path)
                        }
                        onDropped: function(drop) {
                            if (root.dragActive) {
                                if (model.isDir
                                        && root.dropDraggedPaths(model.path, drop.proposedAction))
                                    drop.acceptProposedAction()
                            } else if (root.dropExternalPaths(
                                           drop, model.isDir ? model.path : root.backend.current_path)) {
                                drop.acceptProposedAction()
                            }
                        }
                    }

                    MouseArea {
                        id: gridMouse
                        anchors.fill: parent
                        acceptedButtons: Qt.LeftButton | Qt.RightButton
                        hoverEnabled: true

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
                            if (!root.backend)
                                return
                            if (root.dragActive || root.selectingByMouse)
                                return

                            preventStealing = true
                            dragging = true
                            dragStarted = false
                            suppressClick = false
                            pressModifiers = mouse.modifiers
                            dragOrigin = Qt.point(mouse.x, mouse.y)
                            pressedSelected = root.isSelected(model.path)
                            isMarqueeGesture = !pressedSelected && (mouse.x < 6 || mouse.x > width - 6 || mouse.y < 6 || mouse.y > height - 6)

                            gridView.currentIndex = index
                            gridView.forceActiveFocus()
                            root.setDragHoverPath("")

                            if (!isMarqueeGesture && !pressedSelected) {
                                if (mouse.modifiers & Qt.ShiftModifier)
                                    root.extendSelection(index)
                                else if (mouse.modifiers & Qt.ControlModifier)
                                    root.toggleSelection(model.path, index)
                                else
                                    root.setSingleSelection(model.path, index)
                            }
                        }

                        onPositionChanged: function(mouse) {
                            if (!dragging || !root.backend)
                                return
                            var moved = Math.abs(mouse.x - dragOrigin.x) + Math.abs(mouse.y - dragOrigin.y)
                            if (moved <= Qt.styleHints.startDragDistance)
                                return

                            suppressClick = true

                            if (isMarqueeGesture) {
                                if (!dragStarted) {
                                    dragStarted = true
                                    var originPoint = mapToItem(gridView, dragOrigin.x, dragOrigin.y)
                                    root.beginMouseSelection("grid", originPoint.x, originPoint.y, pressModifiers)
                                }
                                var currentPoint = mapToItem(gridView, mouse.x, mouse.y)
                                root.updateMouseSelectionPointer(currentPoint.x, currentPoint.y)
                                return
                            }

                            if (!dragStarted) {
                                dragStarted = true
                                var paths = root.selectedPaths()
                                if (paths.indexOf(model.path) < 0)
                                    paths = [model.path]
                                root.beginDrag(paths, pressModifiers, model.iconUrl)
                            }

                            if (root.dragActive) {
                                var areaPoint = mapToItem(root, mouse.x, mouse.y)
                                root.updateDragPointer(areaPoint.x, areaPoint.y)
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
                                    root.finishMouseSelection()
                                    dragOrigin = Qt.point(0, 0)
                                    return
                                }
                                isMarqueeGesture = false
                                dragOrigin = Qt.point(0, 0)
                                if (!suppressClick) {
                                    if (mouse.modifiers & Qt.ShiftModifier)
                                        root.extendSelection(index)
                                    else if (mouse.modifiers & Qt.ControlModifier)
                                        root.toggleSelection(model.path, index)
                                    else
                                        root.setSingleSelection(model.path, index)
                                }
                                return
                            }

                            if (dragStarted) {
                                dragStarted = false
                                if (root.dragActive) {
                                    var areaPoint = mapToItem(root, mouse.x, mouse.y)
                                    root.finishInternalDrag(areaPoint.x, areaPoint.y)
                                }
                                dragOrigin = Qt.point(0, 0)
                                return
                            }

                            dragOrigin = Qt.point(0, 0)

                            if (pressedSelected && !suppressClick) {
                                if (mouse.modifiers & Qt.ShiftModifier)
                                    root.extendSelection(index)
                                else if (mouse.modifiers & Qt.ControlModifier)
                                    root.toggleSelection(model.path, index)
                                else
                                    root.setSingleSelection(model.path, index)
                            }
                        }

                        onCanceled: {
                            preventStealing = false
                            dragging = false
                            if (isMarqueeGesture) {
                                if (dragStarted)
                                    root.finishMouseSelection()
                                dragStarted = false
                                isMarqueeGesture = false
                            } else if (dragStarted) {
                                dragStarted = false
                                root.finishDrag()
                            }
                            dragOrigin = Qt.point(0, 0)
                        }

                        onClicked: function(mouse) {
                            if (suppressClick) {
                                suppressClick = false
                                return
                            }
                            if (mouse.button === Qt.RightButton) {
                                if (!root.isSelected(model.path))
                                    root.setSingleSelection(model.path, index)
                                root.openItemMenu(index, mapToItem(root, mouse.x, mouse.y))
                                return
                            }
                            gridView.forceActiveFocus()
                        }
                        onDoubleClicked: {
                            if (root.backend)
                                root.backend.open_index(index)
                        }
                        onWheel: function(wheel) {
                            wheel.accepted = false
                        }
                    }
                }

                DropArea {
                    anchors.fill: parent
                    z: -1
                    keys: ["text/uri-list", "filem/paths"]
                    onEntered: {
                        if (root.dragActive && root.backend)
                            root.setDragHoverPath(root.backend.current_path)
                    }
                    onExited: {
                        if (root.dragActive
                                && root.backend && root.dragHoverPath === root.backend.current_path)
                            root.setDragHoverPath("")
                    }
                    onDropped: function(drop) {
                        if (root.dragActive) {
                            if (root.dropDraggedPaths(
                                        root.backend.current_path, drop.proposedAction))
                                drop.acceptProposedAction()
                        } else if (root.dropExternalPaths(drop, root.backend.current_path)) {
                            drop.acceptProposedAction()
                        }
                    }
                }

                Keys.onPressed: function(event) {
                    if (!root.backend)
                        return
                    if (!(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))
                            && /^[^\u0000-\u001f\u007f]+$/.test(event.text)) {
                        root.requestSearch(event.text)
                        event.accepted = true
                        return
                    }
                    var extend = (event.modifiers & Qt.ShiftModifier) !== 0
                    var keep = (event.modifiers & Qt.ControlModifier) !== 0

                    switch (event.key) {
                    case Qt.Key_Return:
                    case Qt.Key_Enter:
                        if (gridView.currentIndex >= 0) {
                            if (keep)
                                root.backend.open_in_new_tab(gridView.currentIndex)
                            else
                                root.backend.open_index(gridView.currentIndex)
                        }
                        event.accepted = true
                        break
                    case Qt.Key_Backspace:
                        root.backend.go_up()
                        event.accepted = true
                        break
                    case Qt.Key_Escape:
                        if (root.backend.op_active && root.backend.op_cancelable) {
                            root.backend.cancel_operation()
                            event.accepted = true
                        } else if (root.selectionCount > 0) {
                            root.clearSelection()
                            event.accepted = true
                        }
                        break
                    case Qt.Key_Left:
                        root.moveGrid(-1, 0, extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_Right:
                        root.moveGrid(1, 0, extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_Up:
                        root.moveGrid(0, -1, extend, keep)
                        event.accepted = true
                        break
                    case Qt.Key_Down:
                        root.moveGrid(0, 1, extend, keep)
                        event.accepted = true
                        break
                    }
                }

                MouseArea {
                    id: gridBackgroundMouse
                    anchors.fill: parent
                    z: -1
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    preventStealing: true

                    property point pressPoint: Qt.point(0, 0)
                    property bool moved: false

                    onPressed: function(mouse) {
                        if (mouse.button === Qt.RightButton) {
                            var rootPt = mapToItem(root, mouse.x, mouse.y)
                            root.openBackgroundMenu(rootPt.x, rootPt.y)
                            return
                        }
                        pressPoint = Qt.point(mouse.x, mouse.y)
                        moved = false
                        root.beginMouseSelection("grid", mouse.x, mouse.y, mouse.modifiers)
                    }

                    onPositionChanged: function(mouse) {
                        if (!pressed)
                            return
                        if (Math.abs(mouse.x - pressPoint.x) > 3 || Math.abs(mouse.y - pressPoint.y) > 3)
                            moved = true
                        root.updateMouseSelectionPointer(mouse.x, mouse.y)
                    }

                    onReleased: function(mouse) {
                        if (mouse.button !== Qt.LeftButton)
                            return
                        if (!moved && !(mouse.modifiers & (Qt.ControlModifier | Qt.ShiftModifier))) {
                            root.clearSelection()
                            gridView.currentIndex = -1
                            gridView.forceActiveFocus()
                        }
                        root.finishMouseSelection()
                    }

                    onCanceled: root.finishMouseSelection()
                }
            }

            MouseArea {
                id: viewMarqueeStart
                readonly property var currentView: root.gridMode ? gridView
                                                   : root.columnMode ? columnListView : listView
                readonly property real contentBottomY: {
                    if (!root.backend)
                        return 0
                    if (root.gridMode) {
                        var cols = Math.max(1, root.gridColumnCount)
                        var rows = Math.ceil(root.backend.item_count / cols)
                        return (rows * root.gridCellHeight) - currentView.contentY
                    } else {
                        return (root.backend.item_count * root.effectiveRowHeight) - currentView.contentY
                    }
                }
                readonly property real emptyTop: Math.max(0, Math.min(currentView.height, contentBottomY))
                x: 0
                y: emptyTop
                width: currentView.width
                height: Math.max(0, currentView.height - emptyTop)
                z: 5
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                preventStealing: true
                enabled: height > 0

                property point pressPoint: Qt.point(0, 0)
                property bool moved: false

                onPressed: function(mouse) {
                    if (mouse.button === Qt.RightButton) {
                        var rootPt = mapToItem(root, mouse.x, mouse.y)
                        root.openBackgroundMenu(rootPt.x, rootPt.y)
                        return
                    }
                    pressPoint = Qt.point(mouse.x, mouse.y)
                    moved = false
                    var point = mapToItem(currentView, mouse.x, mouse.y)
                    root.beginMouseSelection(root.gridMode ? "grid" : root.columnMode ? "column" : "list",
                                             point.x, point.y, mouse.modifiers)
                }
                onPositionChanged: function(mouse) {
                    if (!pressed)
                        return
                    if (Math.abs(mouse.x - pressPoint.x) > 3 || Math.abs(mouse.y - pressPoint.y) > 3)
                        moved = true
                    var point = mapToItem(currentView, mouse.x, mouse.y)
                    root.updateMouseSelectionPointer(point.x, point.y)
                }
                onReleased: function(mouse) {
                    if (mouse.button !== Qt.LeftButton)
                        return
                    if (!moved && !(mouse.modifiers & (Qt.ControlModifier | Qt.ShiftModifier))) {
                        root.clearSelection()
                        currentView.currentIndex = -1
                        currentView.forceActiveFocus()
                    }
                    root.finishMouseSelection()
                }
                onCanceled: root.finishMouseSelection()
            }

            Rectangle {
                id: selectionBox
                z: 50
                visible: root.selectingByMouse
                readonly property var currentView: root.gridMode ? gridView
                                                   : root.columnMode ? columnListView : listView
                x: currentView.x + Math.min(root.selectionOrigin.x, root.selectionCurrent.x)
                y: currentView.y + Math.max(
                    0,
                    Math.min(
                        currentView.height,
                        Math.min(root.selectionOriginContentY - currentView.contentY,
                                 root.selectionCurrent.y)
                    )
                )
                width: Math.abs(root.selectionCurrent.x - root.selectionOrigin.x)
                height: Math.max(
                    0,
                    Math.min(
                        currentView.height,
                        Math.max(root.selectionOriginContentY - currentView.contentY,
                                 root.selectionCurrent.y)
                    ) - Math.max(
                        0,
                        Math.min(
                            currentView.height,
                            Math.min(root.selectionOriginContentY - currentView.contentY,
                                     root.selectionCurrent.y)
                        )
                    )
                )
                color: theme ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, 0.16)
                             : "#337ea7ff"
                border.width: 1
                border.color: theme ? theme.accent : "#7ea7ff"
            }

            Item {
                id: scrollbar
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                width: 12
                z: 6
                visible: activeView().contentHeight > activeView().height + 1

                readonly property real maxContent: Math.max(0, activeView().contentHeight - activeView().height)
                readonly property real thumbHeight: Math.max(
                    28,
                    height * activeView().height / Math.max(activeView().height, activeView().contentHeight)
                )
                readonly property real thumbY: maxContent > 0
                                                ? Math.max(0, Math.min(
                                                    1,
                                                    (activeView().contentY - activeView().originY)
                                                    / maxContent
                                                )) * (height - thumbHeight)
                                                : 0

                Rectangle {
                    x: 4
                    width: 4
                    height: scrollbar.thumbHeight
                    y: scrollbar.thumbY
                    radius: 2
                    color: theme ? theme.muted : "#9299a8"
                    opacity: scrollbarMouse.containsMouse || scrollbarMouse.pressed ? 0.85 : 0.45
                }

                MouseArea {
                    id: scrollbarMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton

                    property bool dragging: false
                    property real grabOffset: 0

                    function moveThumb(y) {
                        var travel = scrollbar.height - scrollbar.thumbHeight
                        if (travel <= 0 || scrollbar.maxContent <= 0)
                            return
                        var target = Math.max(0, Math.min(travel, y - grabOffset))
                        activeView().contentY = activeView().originY
                                + (target / travel) * scrollbar.maxContent
                    }

                    onPressed: function(mouse) {
                        var top = scrollbar.thumbY
                        if (mouse.y >= top && mouse.y <= top + scrollbar.thumbHeight) {
                            grabOffset = mouse.y - top
                        } else {
                            grabOffset = scrollbar.thumbHeight / 2
                            moveThumb(mouse.y)
                        }
                        dragging = true
                    }

                    onPositionChanged: function(mouse) {
                        if (dragging)
                            moveThumb(mouse.y)
                    }

                    onReleased: dragging = false
                    onCanceled: dragging = false

                    onWheel: function(wheel) {
                        if (wheel.modifiers & Qt.ControlModifier) {
                            if (wheel.angleDelta.y > 0)
                                root.zoomIn()
                            else if (wheel.angleDelta.y < 0)
                                root.zoomOut()
                            wheel.accepted = true
                            return
                        }
                        var wheelDelta = wheel.pixelDelta.y !== 0
                                         ? wheel.pixelDelta.y
                                         : (wheel.angleDelta.y / 120) * 48
                        root.scrollByWheel(activeView(), wheelDelta)
                        wheel.accepted = true
                    }
                }
            }

            Text {
                anchors.centerIn: parent
                width: Math.min(parent.width - 48, 460)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                visible: root.backend
                         && !root.backend.loading
                         && root.backend.item_count === 0
                         && root.backend.error === ""
                text: root.backend && root.backend.search_mode
                      ? (root.backend.recent_view
                         ? "No matches in Recent.\nRefine the query, or press Esc to return."
                         : "No matches in this folder.\nRefine the query, or press Esc to return.")
                      : (root.backend && root.backend.recent_view
                         ? "No recent files.\nOpen a file from the file manager to see it here."
                      : (root.backend && root.backend.in_trash
                         ? "The Trash is empty."
                         : "This folder is empty.\nPress Ctrl+Shift+N for a new folder, or Ctrl+H to show hidden files."))
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                lineHeight: 1.35
            }

        }
    }

    ContextMenu {
        id: menu
        theme: root.theme
        entries: []
    }

    Timer {
        id: scrollSave
        interval: 220
        repeat: false
        onTriggered: root.persistState()
    }

    Connections {
        target: root.backend

        function onCurrent_path_changed() {
            if (!root.backend)
                return
            var path = root.backend.current_path
            if (path === root.lastPath)
                return
            root.lastPath = path
            wheelAnimation.stop()
            root.wheelScrollView = null
            Qt.callLater(function() {
                if (root.dragActive && root.dragPointer.x >= 0)
                    root.updateDragPointer(root.dragPointer.x, root.dragPointer.y)
                // A tab switch emits this signal too; its state is restored by
                // onTabs_changed, so only a real navigation clears selection.
                if (root.backend.active_tab_id !== root.boundTabId)
                    return
                if (root.dragActive)
                    return
                root.clearSelection()
                root.pendingScrollRestore = null
                root.pendingStartPath = path
                root.finishPendingScrollPositioning()
            })
        }

        function onLoading_changed() {
            root.finishPendingScrollPositioning()
        }

        function onItem_count_changed() {
            if (!root.backend)
                return
            wheelAnimation.stop()
            root.wheelScrollView = null
            var count = root.backend.item_count
            var view = root.activeView()
            if (count <= 0) {
                view.currentIndex = -1
            } else if (view.currentIndex < 0) {
                view.currentIndex = 0
            } else if (view.currentIndex >= count) {
                view.currentIndex = count - 1
            }
            root.pruneSelection()
        }

        function onView_changed() {
            if (!root.backend || root.lastViewMode === root.backend.view_mode)
                return
            root.lastViewMode = root.backend.view_mode
            Qt.callLater(function() {
                root.activeView().forceActiveFocus()
            })
        }

        function onTabs_changed() {
            if (!root.backend)
                return
            var id = root.backend.active_tab_id
            if (id === root.boundTabId)
                return
            root.boundTabId = id
            root.restoreTab(id)
        }

        function onClipboard_changed() {
            var map = {}
            if (root.backend && root.backend.clipboard_cut) {
                var paths = root.backend.clipboard_paths
                for (var i = 0; i < paths.length; i++)
                    map[paths[i]] = true
            }
            root.cutMap = map
        }

        function onRenaming_changed() {
            if (root.backend.renaming_path === "")
                activeView().forceActiveFocus()
        }
    }

    Item {
        id: dragGhost
        z: 100
        visible: root.dragActive && root.dragPointer.x >= 0 && root.dragPointer.y >= 0
        x: Math.max(0, Math.min(root.width - width, root.dragPointer.x + 14))
        y: Math.max(0, Math.min(root.height - height, root.dragPointer.y + 14))

        Rectangle {
            width: ghostRow.width + 16
            height: ghostRow.height + 10
            radius: theme ? Math.min(6, theme.radius) : 6
            color: theme ? theme.surface_alt : "#1c202a"
            border.width: 1
            border.color: theme ? theme.accent : "#8ab4f8"
            opacity: 0.92

            Row {
                id: ghostRow
                anchors.centerIn: parent
                spacing: 6

                Image {
                    width: 20
                    height: 20
                    anchors.verticalCenter: parent.verticalCenter
                    source: root.dragImageSource
                    sourceSize.width: 20
                    sourceSize.height: 20
                    fillMode: Image.PreserveAspectFit
                    visible: root.dragImageSource !== ""
                }

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: {
                        var count = root.dragPaths.length
                        if (count <= 1) {
                            var name = count === 1 ? root.dragPaths[0].split("/").pop() : "1 item"
                            return name.length > 24 ? name.substring(0, 22) + "\u2026" : name
                        }
                        return count + " items"
                    }
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    font.weight: Font.Medium
                }
            }
        }
    }

    Component.onCompleted: {
        if (root.backend) {
            root.boundTabId = root.backend.active_tab_id
            root.lastPath = root.backend.current_path
            root.restoreTab(root.boundTabId)
        }
    }
}
