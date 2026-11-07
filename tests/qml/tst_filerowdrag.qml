import QtQuick
import QtTest
import "../../qml/components"

Item {
    id: root
    width: 640
    height: 120

    property int dragStartCount: 0
    property bool nativeDragStarted: false
    property bool columnMode: false

    QtObject {
        id: fakeBackend
        property string view_mode: root.columnMode ? "column" : "list"
        property bool search_mode: false
        property bool virtual_collection: false
        property string renaming_path: ""
        property bool show_size: true
        property bool show_modified: true
        property bool show_kind: true
        property bool recent_view: false
        property bool tag_collection_view: false
        property bool single_click_open: false
    }

    Item {
        id: fakeArea
        width: root.width
        height: root.height
        property bool dragActive: false
        property bool selectingByMouse: false
        property bool gridMode: false
        property bool columnMode: root.columnMode
        property real zoom: 1
        property int hoverAnimationDuration: 0
        property int effectiveRowHeight: 36
        property var dragPaths: []
        property var selected: ({})
        property point dragPointer: Qt.point(-1, -1)

        function activeView() { return rowList }
        function isSelected(path) { return selected[path] === true }
        function isClipboardItem(path) { return false }
        function isMarqueeStart(x, y, width, height) {
            return x < 6 || x > width - 6 || y < 6 || y > height - 6
        }
        function nameWidth(width) { return width - 300 }
        function setDragHoverPath(path) {}
        function setSingleSelection(path, index) { selected = ({[path]: true}) }
        function extendSelection(index) {}
        function toggleSelection(path, index) {
            var next = ({})
            for (var key in selected)
                next[key] = selected[key]
            if (next[path])
                delete next[path]
            else
                next[path] = true
            selected = next
        }
        function beginDrag(path, modifiers, image, point) {
            root.dragStartCount++
            dragPaths = []
            for (var selectedPath in selected) {
                if (selected[selectedPath])
                    dragPaths.push(selectedPath)
            }
            if (dragPaths.indexOf(path) < 0)
                dragPaths = [path]
            dragPointer = point
            dragActive = true
            Qt.callLater(function() {
                if (fakeArea.dragActive && !root.nativeDragStarted)
                    testDragSource.Drag.startDrag(Qt.CopyAction | Qt.MoveAction)
            })
        }
        function updateDragPointer(x, y) {}
        function finishPointerDrag(x, y) { dragActive = false }
        function cancelPointerDrag() { dragActive = false }
        function beginMouseSelection(mode, x, y, modifiers) {}
        function updateMouseSelectionPointer(x, y) {}
        function finishMouseSelection() {}
        function clearSelection() { selected = ({}) }
        function openItemMenu(index, point) {}
        function openBackgroundMenu(x, y) {}

        Item {
            id: testDragSource
            objectName: "testDragSource"
            width: 48
            height: 48
            x: fakeArea.dragPointer.x - width / 2
            y: fakeArea.dragPointer.y - height / 2
            z: -1
            visible: fakeArea.dragActive

            Drag.active: fakeArea.dragActive
            Drag.dragType: Drag.None
            Drag.keys: ["text/uri-list", "filem/paths"]
            Drag.mimeData: ({
                "text/uri-list": "file:///source/item\r\n",
                "filem/paths": "/source/item"
            })
            Drag.supportedActions: Qt.CopyAction | Qt.MoveAction
            Drag.onDragStarted: root.nativeDragStarted = true
        }
    }

    ListView {
        id: rowList
        anchors.fill: parent
        model: ListModel {
            ListElement {
                path: "/source/item"
                name: "item"
                isDir: false
                extension: ""
                sizeText: "1 KB"
                modifiedMs: 1
                isSymlink: false
                iconUrl: ""
                treeDepth: 0
                treeExpandable: false
                treeExpanded: false
                tags: []
                kind: "File"
                displayPath: ""
            }
        }
        delegate: FileRow {
            objectName: "dragTestRow"
            theme: null
            backend: fakeBackend
            area: fakeArea
        }

        TapHandler {
            acceptedButtons: Qt.LeftButton
            onTapped: function(point) {
                if (rowList.indexAt(point.position.x,
                                    point.position.y + rowList.contentY) < 0)
                    fakeArea.clearSelection()
            }
        }

        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: function(point) {
                fakeArea.openBackgroundMenu(point.position.x, point.position.y)
            }
        }
    }

    TestCase {
        id: test
        name: "FileRowDrag"
        when: windowShown

        function init() {
            root.dragStartCount = 0
            root.nativeDragStarted = false
            fakeArea.dragActive = false
            fakeArea.dragPaths = []
            fakeArea.selected = ({})
            root.columnMode = false
        }

        function dragRow() {
            var row = findChild(root, "dragTestRow")
            verify(row !== null)
            var mouseArea = findChild(row, "fileRowMouseArea")
            verify(mouseArea !== null)
            mousePress(mouseArea, 320, 18, Qt.LeftButton)
            mouseMove(mouseArea, 350, 18, 20, Qt.LeftButton)
            compare(root.dragStartCount, 1)
            compare(fakeArea.dragPaths, ["/source/item"])
            tryVerify(function() { return root.nativeDragStarted })
            mouseRelease(mouseArea, 350, 18, Qt.LeftButton)
        }

        function test_listMode() {
            dragRow()
        }

        function test_columnMode() {
            root.columnMode = true
            wait(0)
            dragRow()
        }

        function test_draggingSelectedRowWithControlKeepsGroup() {
            fakeArea.selected = ({"/source/item": true, "/source/other": true})
            var row = findChild(root, "dragTestRow")
            var mouseArea = findChild(row, "fileRowMouseArea")
            mousePress(mouseArea, 320, 18, Qt.LeftButton, Qt.ControlModifier)
            mouseMove(mouseArea, 350, 18, 20, Qt.LeftButton)
            compare(root.dragStartCount, 1)
            compare(fakeArea.dragPaths, ["/source/item", "/source/other"])
            mouseRelease(mouseArea, 350, 18, Qt.LeftButton)
        }
    }
}
