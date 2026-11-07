import QtQuick
import QtQuick.Window
import QtQuick.Layouts
import QtQuick.Dialogs
import Filem 1.0
import "components"
import "components/MenuManager.js" as MenuManager

Window {
    id: root

    width: 1120
    height: 760
    minimumWidth: 760
    minimumHeight: 480
    visible: true
    property bool primaryWindow: true
    property string requestedPath: ""
    property var windowComponent: null
    title: "Filem \u2014 " + (backend.recent_view
                              ? "Recent"
                              : (backend.tag_collection_view
                                 ? backend.tag_collection_label
                                 : (backend.current_path !== "" ? backend.current_path : "Home")))
    color: "transparent"

    Binding {
        target: root.contentItem
        property: "opacity"
        value: theme.opacity
    }

    Backend { id: backend }
    Theme { id: theme }
    Lifecycle { id: lifecycle }

    property string dismissedWarning: ""
    property bool searchOpen: false
    property int contextMenuRevision: 0
    property bool previewVisible: backend ? backend.preview_visible : true
    property string promptMode: ""
    property int promptBookmarkIndex: -1
    property string pendingAction: ""
    property string pendingEjectPath: ""
    property string pendingEjectDevice: ""
    property string pendingUnlockDevice: ""
    property string lastConfirmedText: ""
    property string pendingOpenWithPath: ""
    property var pendingOpenWithPaths: []
    property string pendingExtractPath: ""
    property var pendingTagPaths: []
    property var pendingCompressPaths: []

    readonly property bool dialogOpen: promptDialog.open || confirmDialog.open || conflictDialog.open
                                        || unlockDialog.open || openWithDialog.open
                                        || tagPicker.open
                                        || compressDialog.open
                                        || batchRenameDialog.open
                                        || appearanceDialog.open
                                        || performanceDialog.open
                                        || keyboardShortcutsDialog.open
                                        || transferManager.open
    readonly property bool typing: toolbar.editing
                                    || backend.renaming_path !== ""
                                    || quickLook.opened
                                    || dialogOpen

    Component.onCompleted: {
        lifecycle.window_opened()
        width = backend.window_width
        height = backend.window_height
        theme.start()
        backend.start(requestedPath !== "" ? requestedPath : initialPath)
        Qt.callLater(function() {
            fileArea.listView.forceActiveFocus()
        })
    }

    ModalDialog {
        id: unlockDialog
        theme: theme
        showInput: true
        secretInput: true
        showCancel: true
        title: "Unlock encrypted volume"
        placeholder: "Passphrase"
        confirmText: "Unlock"
        onConfirmed: function(passphrase) {
            backend.unlock_device(root.pendingUnlockDevice, passphrase)
            root.pendingUnlockDevice = ""
            inputText = ""
            root.focusList()
        }

        onCanceled: {
            root.pendingUnlockDevice = ""
            root.focusList()
        }
    }

    BatchRenameDialog {
        id: batchRenameDialog
        theme: theme
        backend: backend
        onDismissed: root.focusList()
    }

    onClosing: function(close) {
        backend.save_window(width, height)
        backend.save_session()
        backend.flush_recent_history()
        backend.pause_for_background()
        close.accepted = false
        visible = false
        lifecycle.window_closed()
        if (!primaryWindow)
            Qt.callLater(function() { root.destroy() })
    }

    // ---- helpers ------------------------------------------------------

    function openLaunchedPath(path) {
        if (primaryWindow && !visible && lifecycle.visible_window_count() === 0) {
            visible = true
            raise()
            requestActivate()
            lifecycle.window_opened()
            if (path !== "")
                backend.navigate(path)
            else
                backend.resume_from_background()
            return
        }
        openAdditionalWindow(path)
    }

    function openAdditionalWindow(path) {
        if (!windowComponent)
            windowComponent = Qt.createComponent(Qt.resolvedUrl("Main.qml"))
        if (windowComponent.status === Component.Ready) {
            var window = windowComponent.createObject(null, {
                "primaryWindow": false,
                "requestedPath": path
            })
            if (!window)
                console.error("Could not create a Filem window:", windowComponent.errorString())
        } else if (windowComponent.status === Component.Error) {
            console.error("Could not load a Filem window:", windowComponent.errorString())
        } else {
            windowComponent.statusChanged.connect(function() {
                if (windowComponent.status === Component.Ready)
                    root.openAdditionalWindow(path)
                else if (windowComponent.status === Component.Error)
                    console.error("Could not load a Filem window:", windowComponent.errorString())
            })
        }
    }

    function focusList() {
        fileArea.listView.forceActiveFocus()
    }

    function selectedOrCurrent() {
        var paths = fileArea.selectedPaths()
        if (paths.length > 0)
            return paths
        if (fileArea.currentPath !== "")
            return [fileArea.currentPath]
        return []
    }

    function baseName(path) {
        var parts = path.split("/")
        return parts[parts.length - 1] || path
    }

    function describePaths(paths) {
        var names = []
        for (var i = 0; i < paths.length && i < 4; i++)
            names.push(baseName(paths[i]))
        var text = names.join(", ")
        if (paths.length > 4)
            text += " and " + (paths.length - 4) + " more"
        return text
    }

    function openSearch() {
        searchOpen = true
        toolbar.beginSearch()
    }

    function openSearchForText(query) {
        searchOpen = true
        toolbar.beginSearch(query)
    }

    function closeSearch() {
        searchOpen = false
        toolbar.cancelSearch()
    }

    function hasActiveTransfer() {
        var jobs = backend ? backend.transfer_jobs : []
        for (var i = 0; i < jobs.length; i++) {
            if (jobs[i].status === "running" || jobs[i].status === "queued")
                return true
        }
        return false
    }

    function closeContextMenu() {
        return MenuManager.closeTopMenu()
    }

    function renameCurrent() {
        var paths = fileArea.selectedPaths()
        if (paths.length > 1) {
            askBatchRename()
            return
        }
        var path = paths.length === 1 ? paths[0] : fileArea.currentPath
        if (path !== "")
            backend.begin_rename(path)
    }

    function deleteSelection() {
        if (backend.in_trash) {
            confirmPermanentDelete()
            return
        }
        var paths = selectedOrCurrent()
        if (paths.length === 0)
            return
        backend.trash_paths(paths)
        fileArea.clearSelection()
    }

    function confirmPermanentDelete() {
        var paths = selectedOrCurrent()
        if (paths.length === 0)
            return
        pendingAction = "delete"
        confirmDialog.title = paths.length === 1
                             ? "Delete this item permanently?"
                             : "Delete " + paths.length + " items permanently?"
        confirmDialog.message = describePaths(paths)
                                + "\n\nThis removes them from the disk immediately. "
                                + "They will not go to the Trash, and Filem cannot undo this."
        confirmDialog.confirmText = "Delete Permanently"
        confirmDialog.destructive = true
        confirmDialog.open = true
    }

    function askEmptyTrash() {
        pendingAction = "empty-trash"
        confirmDialog.title = "Empty the Trash?"
        confirmDialog.message = "Everything in the Trash will be deleted from the disk. "
                                + "This cannot be undone."
        confirmDialog.confirmText = "Empty Trash"
        confirmDialog.destructive = true
        confirmDialog.open = true
    }

    function runConfirmed() {
        if (pendingAction === "delete") {
            var paths = selectedOrCurrent()
            if (paths.length > 0)
                backend.delete_paths(paths, true)
            fileArea.clearSelection()
        } else if (pendingAction === "empty-trash") {
            backend.empty_trash()
        } else if (pendingAction === "eject") {
            backend.eject_device(pendingEjectPath, pendingEjectDevice)
        }
        pendingAction = ""
        pendingEjectPath = ""
        pendingEjectDevice = ""
    }

    function askNetworkLocation() {
        promptMode = "network"
        promptDialog.title = "Connect to Network Location"
        promptDialog.message = "Enter a server address. Authentication is handled by the system when supported."
        promptDialog.placeholder = "smb://server/share"
        promptDialog.inputText = ""
        promptDialog.confirmText = "Connect"
        promptDialog.destructive = false
        promptDialog.open = true
    }

    function askUnlock(device, label) {
        pendingUnlockDevice = device
        unlockDialog.title = "Unlock " + label
        unlockDialog.message = "The passphrase is sent only to UDisks2 for this explicit unlock request."
        unlockDialog.inputText = ""
        unlockDialog.open = true
    }

    function askEject(path, device, label) {
        pendingAction = "eject"
        pendingEjectPath = path
        pendingEjectDevice = device
        confirmDialog.title = "Eject " + label + "?"
        confirmDialog.message = "Filem will unmount the volume before powering off the device."
        confirmDialog.confirmText = "Eject"
        confirmDialog.destructive = true
        confirmDialog.open = true
    }

    function askNewFolder() {
        if (backend.recent_view)
            return
        promptMode = "folder"
        promptDialog.title = "New Folder"
        promptDialog.message = "Created in " + backend.current_path
        promptDialog.inputText = backend.suggest_name(backend.current_path, "New Folder")
        promptDialog.confirmText = "Create"
        promptDialog.destructive = false
        promptDialog.open = true
    }

    function askNewFile() {
        if (backend.recent_view)
            return
        promptMode = "file"
        promptDialog.title = "New File"
        promptDialog.message = "Created empty in " + backend.current_path
        promptDialog.inputText = backend.suggest_name(backend.current_path, "New File")
        promptDialog.confirmText = "Create"
        promptDialog.destructive = false
        promptDialog.open = true
    }

    function askRenameBookmark(index, label) {
        promptMode = "bookmark"
        promptBookmarkIndex = index
        promptDialog.title = "Rename Bookmark"
        promptDialog.message = ""
        promptDialog.inputText = label
        promptDialog.confirmText = "Rename"
        promptDialog.destructive = false
        promptDialog.open = true
    }

    function askBatchRename() {
        var selected = fileArea.selectedPaths()
        if (selected.length < 2 || backend.in_trash)
            return
        Qt.callLater(function() {
            batchRenameDialog.openForPaths(selected)
        })
    }

    function handlePrompt(text) {
        var name = text.trim()
        if (name === "")
            return
        if (promptMode === "network") {
            backend.connect_network_location(name)
        } else if (promptMode === "folder") {
            var path = backend.create_folder(backend.current_path, name)
            if (path !== "")
                backend.begin_rename(path)
        } else if (promptMode === "file") {
            var file = backend.create_file(backend.current_path, name)
            if (file !== "")
                backend.begin_rename(file)
        } else if (promptMode === "bookmark") {
            backend.rename_bookmark(promptBookmarkIndex, name)
        }
        promptMode = ""
        promptBookmarkIndex = -1
    }

    function conflictList() {
        var names = []
        var list = backend.conflict_names
        for (var i = 0; i < list.length; i++)
            names.push(list[i])
        return names.join(", ")
    }

    // ---- shortcuts ----------------------------------------------------

    Shortcut { sequence: "Ctrl+L"; onActivated: toolbar.beginEdit() }
    Shortcut { sequence: "Ctrl+F"; onActivated: root.openSearch() }
    Shortcut {
        sequence: "Alt+Return"
        enabled: !root.typing && !propertiesDialog.opened && fileArea.currentPath !== ""
        onActivated: {
            var paths = fileArea.selectedPaths()
            if (paths.length === 0)
                paths = [fileArea.currentPath]
            backend.show_properties_for_paths(paths)
        }
    }
    Shortcut {
        sequence: "Ctrl+Alt+B"
        onActivated: backend.toggle_tab_bar()
    }
    Shortcut {
        sequence: "Ctrl+T"
        enabled: !root.typing
        onActivated: backend.new_tab("")
    }
    Shortcut {
        sequence: "Ctrl+W"
        enabled: !root.typing
        onActivated: backend.close_tab(backend.active_tab)
    }
    Shortcut {
        sequence: "Ctrl+Shift+T"
        enabled: !root.typing
        onActivated: backend.reopen_tab()
    }
    Shortcut {
        sequence: "Ctrl+Tab"
        enabled: !root.typing
        onActivated: backend.next_tab()
    }
    Shortcut {
        sequence: "Ctrl+Shift+Tab"
        enabled: !root.typing
        onActivated: backend.prev_tab()
    }
    Shortcut {
        sequence: "Ctrl+Shift+N"
        enabled: !root.typing
        onActivated: root.askNewFolder()
    }
    Shortcut {
        sequence: "Ctrl+C"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.copy_paths(fileArea.selectedPaths())
    }
    Shortcut {
        sequence: "Ctrl+X"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.cut_paths(fileArea.selectedPaths())
    }
    Shortcut {
        sequence: "Ctrl+V"
        enabled: !root.typing
        onActivated: backend.paste(backend.current_path, "ask")
    }
    Shortcut {
        sequence: "Ctrl+A"
        enabled: !root.typing
        onActivated: fileArea.selectAll()
    }
    Shortcut {
        sequence: "F2"
        enabled: !root.typing
        onActivated: root.renameCurrent()
    }
    Shortcut {
        sequence: "Delete"
        enabled: !root.typing
        onActivated: root.deleteSelection()
    }
    Shortcut {
        sequence: "Shift+Delete"
        enabled: !root.typing
        onActivated: root.confirmPermanentDelete()
    }
    Shortcut {
        sequence: "Ctrl+Z"
        enabled: !root.typing
        onActivated: backend.undo()
    }
    Shortcut {
        sequence: "Ctrl+Shift+Z"
        enabled: !root.typing
        onActivated: backend.redo()
    }
    Shortcut {
        sequence: "Ctrl+H"
        enabled: !root.typing
        onActivated: backend.toggle_hidden()
    }
    Shortcut {
        sequence: "Ctrl+R"
        enabled: !root.typing
        onActivated: backend.refresh()
    }
    Shortcut {
        sequence: "F5"
        enabled: !root.typing
        onActivated: backend.refresh()
    }
    Shortcut {
        sequence: "Alt+F5"
        enabled: !root.typing
        onActivated: backend.refresh()
    }
    Shortcut {
        sequence: "Ctrl+P"
        enabled: !root.typing
        onActivated: backend.toggle_preview()
    }
    Shortcut {
        sequence: "Space"
        enabled: quickLook.opened || (!root.typing && fileArea.currentPath !== "")
        onActivated: {
            if (quickLook.opened) {
                quickLook.close()
            } else {
                quickLook.openFor(
                    fileArea.currentPath,
                    [],
                    fileArea.activeView().currentIndex
                )
            }
        }
    }
    Shortcut {
        sequence: "Ctrl+B"
        onActivated: backend.toggle_sidebar()
    }
    Shortcut {
        sequence: "Ctrl+D"
        enabled: !root.typing && backend && !backend.recent_view
        onActivated: backend.add_bookmark(backend.current_path, "")
    }
    Shortcut {
        sequence: "T, 1"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.set_tag(fileArea.selectedPaths(), 0)
    }
    Shortcut {
        sequence: "T, 2"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.set_tag(fileArea.selectedPaths(), 1)
    }
    Shortcut {
        sequence: "T, 3"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.set_tag(fileArea.selectedPaths(), 2)
    }
    Shortcut {
        sequence: "T, 4"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.set_tag(fileArea.selectedPaths(), 3)
    }
    Shortcut {
        sequence: "T, 5"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.set_tag(fileArea.selectedPaths(), 4)
    }
    Shortcut {
        sequence: "T, 6"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.set_tag(fileArea.selectedPaths(), 5)
    }
    Shortcut {
        sequence: "T, 7"
        enabled: !root.typing && fileArea.selectionCount > 0
        onActivated: backend.set_tag(fileArea.selectedPaths(), 6)
    }
    Shortcut {
        sequence: "Ctrl+Shift+D"
        onActivated: theme.toggle_mode()
    }
    Shortcut {
        sequence: "Ctrl+="
        enabled: !root.typing
        onActivated: fileArea.zoomIn()
    }
    Shortcut {
        sequence: "Ctrl+Plus"
        enabled: !root.typing
        onActivated: fileArea.zoomIn()
    }
    Shortcut {
        sequence: "Ctrl+-"
        enabled: !root.typing
        onActivated: fileArea.zoomOut()
    }
    Shortcut {
        sequence: "Ctrl+Minus"
        enabled: !root.typing
        onActivated: fileArea.zoomOut()
    }
    Shortcut {
        sequence: "Ctrl+0"
        enabled: !root.typing
        onActivated: fileArea.resetZoom()
    }
    Shortcut {
        sequence: "Alt+Left"
        enabled: !root.typing
        onActivated: backend.go_back()
    }
    Shortcut {
        sequence: "Alt+Right"
        enabled: !root.typing
        onActivated: backend.go_forward()
    }
    Shortcut {
        sequence: "Alt+Up"
        enabled: !root.typing
        onActivated: backend.go_up()
    }
    Shortcut {
        sequence: "Alt+1"
        enabled: backend && backend.sidebar_visible
        onActivated: sidebar.placesView.forceActiveFocus()
    }
    Shortcut {
        sequence: "Alt+2"
        onActivated: root.focusList()
    }
    Shortcut {
        sequence: "Escape"
        enabled: keyboardShortcutsDialog.open || performanceDialog.open
        || transferManager.open || appearanceDialog.open
                 || (root.contextMenuRevision >= 0
                     && (MenuManager.hasOpenMenu() || !root.typing))
        onActivated: {
            if (keyboardShortcutsDialog.open) {
                keyboardShortcutsDialog.close()
            } else if (performanceDialog.open) {
                performanceDialog.close()
            } else if (transferManager.open) {
                transferManager.close()
            } else if (appearanceDialog.open) {
                appearanceDialog.close()
            } else if (root.closeContextMenu()) {
                return
            } else if (root.searchOpen) {
                root.closeSearch()
            } else if (backend.op_active && backend.op_cancelable) {
                backend.cancel_operation()
            } else if (root.hasActiveTransfer()) {
                backend.cancel_operation()
            } else if (fileArea.selectionCount > 0) {
                fileArea.clearSelection()
            }
        }
    }
    Shortcut { sequence: "Ctrl+Q"; onActivated: Qt.quit() }
    Shortcut {
        sequence: "Ctrl+N"
        onActivated: root.openAdditionalWindow(backend.current_path)
    }

    Timer {
        interval: 150
        repeat: true
        running: root.primaryWindow
        onTriggered: {
            var requests = lifecycle.poll_open_requests()
            for (var i = 0; i < requests.length; i++)
                root.openLaunchedPath(requests[i])
            if (lifecycle.should_exit())
                Qt.quit()
        }
    }

    Timer {
        interval: 600
        repeat: true
        running: true
        onTriggered: {
            backend.flush_pending_watch_events()
            backend.refresh_transfer_jobs()
            backend.refresh_clipboard_state()
        }
    }

    // ---- layout -------------------------------------------------------

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        TabBar {
            Layout.fillWidth: true
            Layout.preferredHeight: backend ? (backend.tab_bar_visible ? 36 : 0) : 0
            visible: backend ? backend.tab_bar_visible : false
            clip: true
            theme: theme
            backend: backend

            Behavior on Layout.preferredHeight {
                NumberAnimation { duration: 160; easing.type: Easing.OutCubic }
            }
        }

        Rectangle {
            id: errorBar
            Layout.fillWidth: true
            Layout.preferredHeight: visible ? 40 : 0
            visible: (backend && backend.error !== "")
                     || (theme.warning !== "" && theme.warning !== root.dismissedWarning)
            color: theme.surface_alt

            Rectangle {
                width: 3
                height: parent.height
                color: theme.danger
            }

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 14
                anchors.right: closeError.left
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                text: backend && backend.error !== "" ? backend.error : theme.warning
                color: theme.danger
                font.family: theme.font_family
                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                elide: Text.ElideRight
            }

            IconButton {
                id: closeError
                anchors.right: parent.right
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                theme: theme
                label: "\u00d7"
                tooltip: "Dismiss"
                onClicked: {
                    if (backend && backend.error !== "")
                        backend.clear_error()
                    else
                        root.dismissedWarning = theme.warning
                }
            }
        }

        Row {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            Sidebar {
                id: sidebar
                height: parent.height
                theme: theme
                backend: backend
                onBookmarkRenameRequested: function(index, label) {
                    root.askRenameBookmark(index, label)
                }
                onEmptyTrashRequested: root.askEmptyTrash()
                onUnlockRequested: function(device, label) { root.askUnlock(device, label) }
                onEjectRequested: function(path, device, label) { root.askEject(path, device, label) }
                onNetworkLocationRequested: root.askNetworkLocation()
            }

            Item {
                id: filePane
                width: parent.width - sidebar.width
                height: parent.height

                Toolbar {
                    id: toolbar
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    height: 48
                    theme: theme
                    backend: backend
                    zoom: fileArea.zoom
                    onDone: root.focusList()
                    onSearchRequested: root.openSearch()
                    onSearchCanceled: root.closeSearch()
                    onNewFolderRequested: root.askNewFolder()
                    onTogglePreview: backend.toggle_preview()
                    onAppearanceRequested: appearanceDialog.open = true
                    onPerformanceRequested: performanceDialog.open = true
                    onShortcutsRequested: keyboardShortcutsDialog.open = true
                }

                Row {
                    id: fileContentRow
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: toolbar.bottom
                    anchors.bottom: parent.bottom
                    spacing: 0

                    FileArea {
                        id: fileArea
                        width: parent.width - (previewPane.shown ? previewPane.width : 0)
                        height: parent.height
                        theme: theme
                        backend: backend
                        externalDropResolver: function(point) {
                            var sidebarPoint = sidebar.mapFromItem(null, point.x, point.y)
                            var destination = sidebar.dropDestinationAt(
                                        sidebarPoint.x, sidebarPoint.y)
                            if (fileArea.dragPaths.indexOf(destination) >= 0)
                                destination = ""
                            sidebar.dropHoverPath = destination
                            return destination
                        }
                        onRequestTrash: root.deleteSelection()
                        onRequestPermanentDelete: root.confirmPermanentDelete()
                        onRequestNewFolder: root.askNewFolder()
                        onRequestNewFile: root.askNewFile()
                        onRequestBatchRename: root.askBatchRename()
                        onRequestEmptyTrash: root.askEmptyTrash()
                        onRequestBookmark: backend.add_bookmark(backend.current_path, "")
                        onRequestOpenWith: function(path, paths) {
                            root.pendingOpenWithPath = path
                            root.pendingOpenWithPaths = paths
                            openWithDialog.fileName = path.split("/").pop()
                            openWithDialog.applications = backend.open_with_apps(path)
                            openWithDialog.open = true
                        }
                        onRequestExtractTo: function(path) {
                            root.pendingExtractPath = path
                            extractFolderDialog.open()
                        }
                        onRequestTag: function(paths) {
                            root.pendingTagPaths = paths
                            tagPicker.open = true
                        }
                        onRequestCompress: function(paths) {
                            root.pendingCompressPaths = paths
                            compressDialog.itemCount = paths.length
                            compressDialog.open = true
                        }
                        onRequestSearch: function(query) {
                            root.openSearchForText(query)
                        }
                    }

                    PreviewPane {
                        id: previewPane
                        shown: backend && backend.preview_visible
                               && fileArea.currentPath !== ""
                        width: shown ? 320 : 0
                        height: parent.height
                        visible: width > 0
                        theme: theme
                        backend: backend
                        fileArea: fileArea

                        Behavior on width {
                            NumberAnimation { duration: 180; easing.type: Easing.OutCubic }
                        }
                    }
                }
            }
        }

        PropertiesDialog {
            id: propertiesDialog
            theme: theme
            backend: backend
            visible: backend && backend.properties_open
        }

        StatusBar {
            Layout.fillWidth: true
            Layout.preferredHeight: 27
            theme: theme
            backend: backend
            fileArea: fileArea
        }
    }

    ProgressPanel {
        theme: theme
        backend: backend
        manager: transferManager
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.rightMargin: 18
        anchors.bottomMargin: 44
        z: 200
    }

    QuickLook {
        id: quickLook
        theme: theme
        backend: backend
        onDismissed: {
            fileArea.activeView().forceActiveFocus()
            if (backend.preview_visible && fileArea.currentPath !== "")
                backend.request_preview(fileArea.currentPath)
        }
    }

    OpenWithDialog {
        id: openWithDialog
        theme: theme
        onChosen: function(appId) {
            backend.open_with_app(appId, root.pendingOpenWithPaths)
            root.pendingOpenWithPath = ""
            root.pendingOpenWithPaths = []
            root.focusList()
        }
        onCanceled: {
            root.pendingOpenWithPath = ""
            root.pendingOpenWithPaths = []
            root.focusList()
        }
    }

    AppearanceDialog {
        id: appearanceDialog
        theme: theme
    }

    KeyboardShortcutsDialog {
        id: keyboardShortcutsDialog
        theme: theme
    }

    PerformanceDialog {
        id: performanceDialog
        theme: theme
        lifecycle: lifecycle
    }

    TransferManager {
        id: transferManager
        theme: theme
        backend: backend
    }

    TagPicker {
        id: tagPicker
        theme: theme
        tags: backend ? backend.tags : []
        onChosen: function(index) {
            backend.toggle_tag(root.pendingTagPaths, index)
            root.pendingTagPaths = []
            root.focusList()
        }
        onCanceled: {
            root.pendingTagPaths = []
            root.focusList()
        }
    }

    CompressDialog {
        id: compressDialog
        theme: theme
        onChosen: function(format, password) {
            backend.compress_paths(root.pendingCompressPaths, format, password)
            root.pendingCompressPaths = []
            root.focusList()
        }
        onCanceled: {
            root.pendingCompressPaths = []
            root.focusList()
        }
    }

    FolderDialog {
        id: extractFolderDialog
        title: "Choose Extraction Location"
        currentFolder: backend
                       ? backend.file_url(backend.recent_view ? "/" : backend.current_path)
                       : ""
        onAccepted: {
            backend.extract_path(root.pendingExtractPath, selectedFolder.toString())
            root.pendingExtractPath = ""
        }
        onRejected: root.pendingExtractPath = ""
    }

    // ---- dialogs ------------------------------------------------------

    ModalDialog {
        id: promptDialog
        theme: theme
        showInput: true
        showCancel: true
        placeholder: "Name"
        onConfirmed: function(text) { root.handlePrompt(text) }
        onCanceled: {
            root.promptMode = ""
            root.promptBookmarkIndex = -1
            root.focusList()
        }
    }

    ModalDialog {
        id: confirmDialog
        theme: theme
        showCancel: true
        onConfirmed: root.runConfirmed()
        onCanceled: {
            root.pendingAction = ""
            root.pendingEjectPath = ""
            root.pendingEjectDevice = ""
            root.focusList()
        }
    }

    ModalDialog {
        id: conflictDialog
        theme: theme
        title: "Items already exist"
        message: (backend ? backend.conflict_detail : "")
                 + "\n\n" + root.conflictList()
        confirmText: "Replace"
        destructive: false
        extraButtons: [
            { text: "Keep Both", value: "keep-both" },
            { text: "Skip", value: "skip" }
        ]
        onConfirmed: backend.confirm_conflicts("replace")
        onExtraChosen: function(value) { backend.confirm_conflicts(value) }
        onCanceled: backend.cancel_conflicts()
    }

    Connections {
        target: backend
        function onConflict_changed() {
            conflictDialog.open = backend.conflict_pending
        }
        function onSearch_changed() {
            if (backend.search_mode && !root.searchOpen) {
                root.searchOpen = true
                toolbar.beginSearch()
            }
        }
    }

    Timer {
        interval: 1400
        running: smokeTest
        repeat: false
        onTriggered: Qt.quit()
    }

    Timer {
        interval: 1800
        running: screenshotPath !== ""
        repeat: false
        onTriggered: {
            var item = root.contentItem
            if (!item) {
                Qt.quit()
                return
            }
            item.grabToImage(function(result) {
                if (!result.saveToFile(screenshotPath))
                    console.warn("Could not save screenshot to " + screenshotPath)
                Qt.quit()
            })
        }
    }
}
