import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    name: "RowDrag"
    when: windowShown
    width: 400
    height: 300
    visible: true

    QtObject {
        id: fakeTheme
        property string font_family: "sans-serif"
        property string mono_font: "monospace"
        property real font_scale: 1
        property real row_height: 34
        property string background: "#0f1115"
        property string surface: "#151820"
        property string hover: "#202531"
        property string foreground: "#e7e9ee"
        property string muted: "#9299a8"
        property string accent: "#8ab4f8"
        property string border: "#292e3a"
        property string selection: "#24364f"
        property string selection_text: "#f2f5fb"
        property string focus: "#8ab4f8"
        property string danger: "#f28b82"
        function tag_color(t) { return "#f28b82" }
    }

    Rectangle {
        id: viewStub
        color: "transparent"
    }

    Item {
        id: fakeArea
        opacity: 0 // pure stub; never rendered
        property bool selectingByMouse: false
        property bool dragActive: false
        property var dragPaths: []
        property string dragHoverPath: ""
        property real zoom: 1
        property real effectiveRowHeight: 34
        property bool gridMode: false
        property bool columnMode: false
        property int hoverAnimationDuration: 100
        property real sizeWidth: 88
        property real modifiedWidth: 150
        property real kindWidth: 130
        property int beginDragCalls: 0
        property string lastBeginPath: ""
        property point lastBeginPointer
        property int finishDragCalls: 0

        function isSelected(path) { return path === "/fake/selected" }
        function isClipboardItem() { return false }
        function activeView() { return viewStub }
        function setDragHoverPath() {}
        function isMarqueeStart(x, y, w, h) { return x < 6 || x > w - 6 || y < 6 || y > h - 6 }
        function extendSelection() {}
        function toggleSelection() {}
        function setSingleSelection() {}
        function beginDrag(itemPath, modifiers, imageSource, pointer) {
            beginDragCalls++
            lastBeginPath = itemPath
            lastBeginPointer = pointer || Qt.point(-1, -1)
            dragActive = true
        }
        function updateDragPointer() {}
        function finishPointerDrag() { finishDragCalls++ }
        function cancelPointerDrag() {}
        function beginMouseSelection() {}
        function updateMouseSelectionPointer() {}
        function finishMouseSelection() {}
        function clearSelection() {}
    }

    ListView {
        id: list
        x: 0
        y: 0
        width: 400
        height: 300
        clip: true
        focus: true
        keyNavigationEnabled: false
        boundsBehavior: Flickable.StopAtBounds
        model: ListModel {
            ListElement {
                name: "selected"
                path: "/fake/selected"
                isDir: false
                sizeText: "12 B"
                modifiedMs: 0
                kind: "file"
                displayPath: ""
                isSymlink: false
                treeDepth: 0
                treeExpanded: false
                treeExpandable: false
                iconUrl: ""
                tags: []
            }
        }
        delegate: FileRow {
            theme: fakeTheme
            backend: null
            area: fakeArea
        }

        DropArea {
            anchors.fill: parent
            z: -1
            keys: ["text/uri-list", "filem/paths"]
        }

        TapHandler {
            acceptedButtons: Qt.LeftButton
        }

        TapHandler {
            acceptedButtons: Qt.RightButton
        }
    }

    function test_selected_row_drag_fires_beginDrag() {
        fakeArea.beginDragCalls = 0
        fakeArea.finishDragCalls = 0
        fakeArea.dragActive = false
        // press near the horizontal centre of the first row
        mousePress(list, 200, 17)
        mouseMove(list, 200, 17)
        mouseMove(list, 240, 17) // 40 px > startDragDistance
        mouseRelease(list, 240, 17)
        verify(fakeArea.beginDragCalls > 0, "beginDrag should be called on row drag")
        compare(fakeArea.lastBeginPath, "/fake/selected")
        verify(fakeArea.finishDragCalls > 0, "finishPointerDrag should be called on release")
        fakeArea.dragActive = false
    }
}
