import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    name: "KeyboardShortcuts"
    when: windowShown
    width: 900
    height: 760

    QtObject {
        id: fakeTheme
        property string font_family: "sans-serif"
        property string mono_font: "monospace"
        property real font_scale: 1
        property real radius: 8
        property color accent: "#8ab4f8"
        property color border: "#292e3a"
        property color foreground: "#e7e9ee"
        property color muted: "#9299a8"
        property color selection: "#24364f"
        property color surface: "#151820"
        property color surface_alt: "#1c202a"
    }

    QtObject {
        id: fakeBackend
        property bool show_hidden: false
        property bool directories_first: true
        property bool single_click_open: false
        property bool tab_bar_visible: true
        property bool show_size: true
        property bool show_modified: true
        property bool show_kind: true
        property string sort_key: "name"
        property bool recent_view: false
        property bool sort_ascending: true
        property bool can_undo: false
        property string undo_label: ""
        property bool can_redo: false
        property string redo_label: ""
        property string current_path: "/home/user"
        property string search_mode: ""
        property string search_query: ""
        property bool preview_visible: false
        property var bookmarks: []
        property bool can_go_back: false
        property bool can_go_forward: false
        property string view_mode: "list"
    }

    KeyboardShortcutsDialog {
        id: dialog
        width: 900
        height: 760
        theme: fakeTheme
        open: true
    }

    Toolbar {
        id: toolbar
        width: 900
        height: 48
        theme: null
        backend: fakeBackend
        onShortcutsRequested: requestObserved = true
    }

    property bool requestObserved: false

    function test_shortcut_groups_cover_application_actions() {
        verify(dialog.sections.length >= 5)
        verify(dialog.hasShortcut("Ctrl+L"))
        verify(dialog.hasShortcut("Ctrl+F"))
        verify(dialog.hasShortcut("Ctrl+Shift+T"))
        verify(dialog.hasShortcut("Ctrl+Shift+N"))
        verify(dialog.hasShortcut("Alt+Enter"))
        verify(dialog.hasShortcut("T, 7"))
        verify(dialog.hasShortcut("Ctrl+0"))
        verify(dialog.hasShortcut("Escape"))
    }

    function test_shortcuts_panel_does_not_scroll_horizontally() {
        var scroll = findChild(dialog, "keyboardShortcutsScroll")
        verify(scroll !== null)
        verify(scroll.contentWidth <= scroll.availableWidth)
    }

    function test_escape_closes_shortcuts_dialog() {
        dialog.open = true
        dialog.forceActiveFocus()
        keyClick(Qt.Key_Escape)
        tryCompare(dialog, "open", false)
    }

    function test_toolbar_menu_opens_shortcuts_dialog() {
        var entries = toolbar.menuEntries()
        var shortcutsEntry = null
        for (var i = 0; i < entries.length; i++) {
            if (entries[i].label === "Keyboard Shortcuts…")
                shortcutsEntry = entries[i]
        }
        verify(shortcutsEntry !== null)
        requestObserved = false
        shortcutsEntry.action()
        compare(requestObserved, true)
    }
}
