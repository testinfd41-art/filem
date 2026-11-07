import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root

    property var theme
    property var backend
    property real zoom: 1.0
    property bool editing: false
    property bool searchEditing: false
    property bool zoomIndicatorVisible: false

    signal done()
    signal searchRequested()
    signal searchCanceled()
    signal newFolderRequested()
    signal togglePreview()
    signal appearanceRequested()
    signal performanceRequested()

    implicitHeight: 48
    color: theme ? theme.surface : "#151820"

    function beginEdit() {
        if (!backend)
            return
        if (backend.search_mode)
            backend.clear_search()
        searchEditing = false
        location.text = backend.recent_view ? "Recent" : backend.current_path
        editing = true
        Qt.callLater(function() {
            location.focusInput()
        })
    }

    function beginSearch(initialQuery) {
        if (!backend)
            return
        searchEditing = true
        location.text = initialQuery === undefined ? backend.search_query : initialQuery
        editing = true
        Qt.callLater(function() {
            location.focusInput()
        })
    }

    function cancelSearch() {
        if (backend)
            backend.clear_search()
        searchEditing = false
        editing = false
        done()
    }

    function leaveSearchInput() {
        editing = false
        searchEditing = false
        done()
    }

    function finish() {
        editing = false
        searchEditing = false
        done()
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

    function check(value) {
        return value ? "\u2713 " : "   "
    }

    onZoomChanged: {
        zoomIndicatorVisible = true
        zoomIndicatorTimer.restart()
    }

    function menuEntries() {
        var entries = []
        entries.push({
            label: root.check(backend.show_hidden) + "Show Hidden Files",
            shortcut: "Ctrl+H",
            action: function() { backend.toggle_hidden() }
        })
        entries.push({
            label: root.check(backend.directories_first) + "Folders First",
            action: function() { backend.toggle_directories_first() }
        })
        entries.push({
            label: root.check(backend.single_click_open) + "Single-Click Opens",
            action: function() { backend.set_single_click_open(!backend.single_click_open) }
        })
        entries.push({
            label: root.check(backend.tab_bar_visible) + "Show Tab Bar",
            shortcut: "Ctrl+Alt+B",
            action: function() { backend.toggle_tab_bar() }
        })
        entries.push({ separator: true })
        entries.push({
            label: root.check(backend.show_size) + "Size Column",
            action: function() { backend.toggle_column("size") }
        })
        entries.push({
            label: root.check(backend.show_modified) + "Modified Column",
            action: function() { backend.toggle_column("modified") }
        })
        entries.push({
            label: root.check(backend.show_kind) + "Kind / Location Column",
            action: function() { backend.toggle_column("kind") }
        })
        entries.push({ separator: true })
        var sortKeys = [
            ["name", "Name"],
            ["extension", "Extension"],
            ["size", "Size"],
            ["modified", backend.recent_view ? "Date Opened" : "Modified"],
            ["kind", backend.recent_view ? "Type" : "Kind"]
        ]
        for (var i = 0; i < sortKeys.length; i++) {
            (function(key, label) {
                entries.push({
                    label: root.check(backend.sort_key === key) + "Sort by " + label,
                    action: function() {
                        backend.set_sort(key, backend.sort_key === key ? !backend.sort_ascending : true)
                    }
                })
            })(sortKeys[i][0], sortKeys[i][1])
        }
        entries.push({ separator: true })
        entries.push({
            label: backend.can_undo ? "Undo " + backend.undo_label : "Nothing to Undo",
            shortcut: "Ctrl+Z",
            enabled: backend.can_undo,
            action: function() { backend.undo() }
        })
        entries.push({
            label: backend.can_redo ? "Redo " + backend.redo_label : "Nothing to Redo",
            shortcut: "Ctrl+Shift+Z",
            enabled: backend.can_redo,
            action: function() { backend.redo() }
        })
        entries.push({ separator: true })
        if (!backend.recent_view) {
            entries.push({
                label: "Bookmark This Folder",
                shortcut: "Ctrl+D",
                action: function() { backend.add_bookmark(backend.current_path, "") }
            })
            entries.push({
                label: "Open Terminal Here",
                action: function() { backend.open_terminal(backend.current_path) }
            })
            entries.push({
                label: "Copy Current Path",
                action: function() { backend.copy_paths_text([backend.current_path]) }
            })
        } else {
            entries.push({
                label: "Clear Recent History",
                action: function() { backend.clear_recent_history() }
            })
        }
        entries.push({ separator: true })
        entries.push({
            label: "Appearance…",
            action: function() { root.appearanceRequested() }
        })
        entries.push({
            label: "Performance…",
            action: function() { root.performanceRequested() }
        })
        entries.push({
            label: "Toggle Dark / Light",
            shortcut: "Ctrl+Shift+D",
            action: function() { root.theme.toggle_mode() }
        })
        entries.push({
            label: "Reload Theme",
            action: function() { root.theme.reload() }
        })
        return entries
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: theme ? theme.border : "#292e3a"
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 8
        anchors.rightMargin: 8
        spacing: 2

        IconButton {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            theme: root.theme
            label: "\u2039"
            tooltip: "Back (Alt+Left)"
            enabled: backend ? backend.can_go_back : false
            onClicked: backend.go_back()
        }

        IconButton {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            theme: root.theme
            label: "\u203a"
            tooltip: "Forward (Alt+Right)"
            enabled: backend ? backend.can_go_forward : false
            onClicked: backend.go_forward()
        }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 2

            Breadcrumbs {
                anchors.fill: parent
                visible: !root.editing
                theme: root.theme
                backend: root.backend
                onEditRequested: root.beginEdit()
            }

            LocationBar {
                id: location
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width
                visible: root.editing
                theme: root.theme
                backend: root.backend
                searchMode: root.searchEditing

                onAccepted: function(path) {
                    var trimmed = path.trim()
                    if (root.searchEditing) {
                        location.runSearch()
                        root.editing = false
                        return
                    }
                    root.finish()
                    if (trimmed !== "")
                        root.backend.navigate(trimmed)
                }
                onCanceled: {
                    if (root.searchEditing)
                        root.searchCanceled()
                    else
                        root.finish()
                }
                onMoveToList: root.leaveSearchInput()
            }
        }

        Text {
            Layout.preferredWidth: root.zoomIndicatorVisible ? 36 : 0
            Layout.alignment: Qt.AlignVCenter
            visible: root.zoomIndicatorVisible
            horizontalAlignment: Text.AlignHCenter
            text: Math.round(root.zoom * 100) + "%"
            color: theme ? theme.muted : "#9299a8"
            font.family: theme ? theme.mono_font : "monospace"
            font.pixelSize: 10 * (theme ? theme.font_scale : 1)
            Accessible.name: "Zoom " + Math.round(root.zoom * 100) + " percent"
        }

        Rectangle {
            Layout.preferredWidth: 96
            Layout.preferredHeight: 32
            Layout.alignment: Qt.AlignVCenter
            radius: theme ? Math.max(9, theme.radius * 0.72) : 9
            color: "transparent"

            RowLayout {
                anchors.fill: parent
                anchors.margins: 1
                spacing: 0

                IconButton {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.maximumWidth: 30
                    Layout.maximumHeight: 30
                    Layout.preferredWidth: 30
                    Layout.preferredHeight: 30
                    theme: root.theme
                    backgroundless: true
                    label: ""
                    tooltip: {
                        if (!backend)
                            return "Switch view"
                        return backend.view_mode === "list" ? "Switch to grid view" : backend.view_mode === "grid" ? "Switch to column view" : "Switch to list view"
                    }
                    active: backend ? backend.view_mode !== "list" : false
                    onClicked: {
                        if (!backend)
                            return
                        var next = backend.view_mode === "list" ? "grid" : backend.view_mode === "grid" ? "column" : "list"
                        backend.set_view_mode(next)
                    }

                    ToolbarGlyph {
                        anchors.centerIn: parent
                        scale: 0.86
                        name: !root.backend || root.backend.view_mode === "list" ? "view-list"
                              : root.backend.view_mode === "grid" ? "view-grid" : "view-columns"
                        color: parent.active
                               ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                               : (root.theme ? root.theme.foreground : "#e7e9ee")
                    }
                }

                IconButton {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.maximumWidth: 30
                    Layout.maximumHeight: 30
                    Layout.preferredWidth: 30
                    Layout.preferredHeight: 30
                    theme: root.theme
                    backgroundless: true
                    label: ""
                    tooltip: "Search (Ctrl+F)"
                    active: backend ? backend.search_mode : false
                    onClicked: root.searchRequested()

                    ToolbarGlyph {
                        anchors.centerIn: parent
                        scale: 0.86
                        name: "search"
                        color: parent.active
                               ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                               : (root.theme ? root.theme.foreground : "#e7e9ee")
                    }
                }

                IconButton {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.maximumWidth: 30
                    Layout.maximumHeight: 30
                    Layout.preferredWidth: 30
                    Layout.preferredHeight: 30
                    theme: root.theme
                    backgroundless: true
                    label: ""
                    tooltip: "Toggle preview pane (Ctrl+P)"
                    active: backend ? backend.preview_visible : false
                    onClicked: root.togglePreview()

                    ToolbarGlyph {
                        anchors.centerIn: parent
                        scale: 0.86
                        name: "preview"
                        color: parent.active
                               ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                               : (root.theme ? root.theme.foreground : "#e7e9ee")
                    }
                }
            }
        }

        IconButton {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            theme: root.theme
            enabled: backend ? !backend.recent_view : false
            label: ""
            tooltip: isBookmarked(backend ? backend.current_path : "")
                     ? "Remove bookmark (Ctrl+D)" : "Bookmark this folder (Ctrl+D)"
            active: isBookmarked(backend ? backend.current_path : "")
            onClicked: backend.toggle_bookmark(backend.current_path)

            ToolbarGlyph {
                anchors.centerIn: parent
                scale: 0.86
                name: "bookmark"
                filled: parent.active
                color: parent.active
                       ? (root.theme ? root.theme.accent : "#8ab4f8")
                       : (root.theme ? root.theme.foreground : "#e7e9ee")
            }
        }

        IconButton {
            id: overflowButton
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            theme: root.theme
            label: "\u22ef"
            tooltip: "More actions"
            onClicked: {
                menu.entries = root.menuEntries()
                var point = mapToItem(root, 0, root.height)
                menu.open(point.x, point.y)
            }
        }
    }

    Timer {
        id: zoomIndicatorTimer
        interval: 1000
        onTriggered: root.zoomIndicatorVisible = false
    }

    ContextMenu {
        id: menu
        theme: root.theme
        entries: []
    }
}
