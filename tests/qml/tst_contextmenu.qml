import QtQuick
import QtTest
import "../../qml/components"
import "../../qml/components/MenuManager.js" as MenuManager

TestCase {
    id: test
    name: "ContextMenuPlacement"
    when: windowShown
    width: 600
    height: 400

    ContextMenu {
        id: menu
    }

    ContextMenu {
        id: nestedMenu
    }

    function assertInsideWindow() {
        var bounds = menu.menuWindow.contentItem
        compare(menu.parent, bounds)
        verify(menu.x >= 8)
        verify(menu.y >= 8)
        verify(menu.x + menu.width <= bounds.width - 8)
        verify(menu.y + menu.height <= bounds.height - 8)
    }

    function test_all_edges_and_corners() {
        menu.menuWindow.width = 600
        menu.menuWindow.height = 400
        wait(30)
        var bounds = menu.menuWindow.contentItem
        menu.entries = [{ label: "Open" }, { label: "Properties" }]
        var points = [
            Qt.point(8, 8),
            Qt.point(bounds.width - 8, 8),
            Qt.point(8, bounds.height - 8),
            Qt.point(bounds.width - 8, bounds.height - 8)
        ]
        for (var i = 0; i < points.length; i++) {
            menu.open(points[i].x, points[i].y)
            wait(30)
            assertInsideWindow()
            if (i < 2)
                verify(menu.y >= points[i].y)
            else
                verify(menu.y + menu.height <= points[i].y + 1)
            menu.close()
        }
    }

    function test_long_menu_is_constrained_and_scrollable() {
        menu.menuWindow.width = 600
        menu.menuWindow.height = 400
        wait(30)
        var bounds = menu.menuWindow.contentItem
        menu.entries = [{ label: "Open" }]
        menu.open(bounds.width / 2, bounds.height / 2)
        wait(30)
        var shortHeight = menu.height
        var entries = []
        for (var i = 0; i < 80; i++)
            entries.push({ label: "Action " + i })
        menu.entries = entries
        wait(30)
        assertInsideWindow()
        verify(menu.height > shortHeight)
        verify(menu.height <= menu.menuWindow.contentItem.height - 16)
        verify(menu.scrollViewport.contentHeight > menu.scrollViewport.height)
        menu.close()
    }

    function test_window_resize_repositions_open_menu() {
        menu.menuWindow.width = 600
        menu.menuWindow.height = 400
        wait(30)
        menu.entries = [{ label: "Open" }, { label: "Properties" }]
        menu.open(menu.menuWindow.contentItem.width - 10, menu.menuWindow.contentItem.height - 10)
        wait(30)
        assertInsideWindow()
        menu.menuWindow.width = 360
        menu.menuWindow.height = 260
        wait(30)
        assertInsideWindow()
        menu.menuWindow.width = 220
        menu.menuWindow.height = 180
        wait(30)
        assertInsideWindow()
        verify(menu.width <= menu.menuWindow.contentItem.width - 16)
        menu.close()
    }

    function test_clicking_menu_action_runs_it_before_dismissal() {
        menu.menuWindow.width = 600
        menu.menuWindow.height = 400
        wait(30)
        var invoked = false
        var actionSawMenuOpen = false
        menu.entries = [{ label: "Open", action: function() {
            actionSawMenuOpen = menu.opened
            invoked = true
        } }]
        menu.open(120, 120)
        wait(30)
        mouseClick(menu, menu.width / 2, 6 + 15)
        compare(invoked, true)
        compare(actionSawMenuOpen, true)
        compare(menu.opened, false)
    }

    function test_clicking_outside_dismisses_menu() {
        menu.entries = [{ label: "Open" }]
        menu.open(120, 120)
        mouseClick(menu.menuWindow.contentItem, 550, 350)
        compare(menu.opened, false)
    }

    function test_escape_closes_only_the_topmost_menu() {
        menu.entries = [{ label: "Open" }]
        nestedMenu.entries = [{ label: "Nested action" }]

        menu.open(120, 120)
        nestedMenu.open(140, 140)
        keyClick(Qt.Key_Escape)
        compare(nestedMenu.opened, false)
        compare(menu.opened, true)
        verify(menu.activeFocus)

        keyClick(Qt.Key_Escape)
        compare(menu.opened, false)
    }

    function test_escape_closes_menu_immediately_after_open() {
        menu.entries = [{ label: "Open" }]
        menu.open(100, 100)
        keyClick(Qt.Key_Escape)
        compare(menu.opened, false)
    }

    function test_menu_stack_changes_notify_window_shortcuts() {
        var window = { contextMenuRevision: 0 }
        var fakeMenu = { menuWindow: window }

        MenuManager.registerMenu(fakeMenu)
        compare(window.contextMenuRevision, 1)
        MenuManager.unregisterMenu(fakeMenu)
        compare(window.contextMenuRevision, 2)
    }
}
