import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    id: test
    name: "DragDrop"
    when: windowShown
    width: 720
    height: 480

    property int resolvedTargetCount: 0

    FileArea {
        id: area
        anchors.fill: parent
        externalDropResolver: function(point) {
            test.resolvedTargetCount++
            return "/destination"
        }
    }

    function init() {
        resolvedTargetCount = 0
        area.dragSession = null
        area.nativeDragUsed = false
    }

    function test_nativeDragReleaseWaitsForPlatformCompletion() {
        area.dragSession = {
            paths: ["/source/item"],
            valid: true,
            consumed: false,
            copy: false,
            autoMove: true,
            sourceDirectory: "/source"
        }
        area.nativeDragUsed = true

        area.finishPointerDrag(20, 20)

        compare(resolvedTargetCount, 0)
        compare(area.dragActive, true)
        area.finishDrag()
        compare(resolvedTargetCount, 1)
        compare(area.dragSession, null)
    }

    function test_nativeDragSurvivesPointerCancellationUntilDragFinishes() {
        area.dragSession = {
            paths: ["/source/item"],
            valid: true,
            consumed: false,
            copy: false,
            autoMove: true,
            sourceDirectory: "/source"
        }
        area.nativeDragUsed = true

        area.cancelPointerDrag()

        compare(area.dragActive, true)
        area.finishDrag()
        compare(area.dragSession, null)
    }

    function test_pointerCancellationDoesNotDropPendingNativeDrag() {
        area.dragSession = {
            paths: ["/source/item"],
            valid: true,
            consumed: false,
            copy: false,
            autoMove: true,
            sourceDirectory: "/source"
        }

        area.cancelPointerDrag()
        area.nativeDragUsed = true
        wait(150)
        compare(area.dragActive, true)
        area.finishDrag()
    }

    function test_pointerCancellationCleansUpIfNativeDragNeverStarts() {
        area.dragSession = {
            paths: ["/source/item"],
            valid: true,
            consumed: false,
            copy: false,
            autoMove: true,
            sourceDirectory: "/source"
        }

        area.cancelPointerDrag()
        tryVerify(function() { return !area.dragActive }, 500)
    }

    function test_dragPayloadPreservesTheSelectedGroupOnce() {
        area.selection = ({"/source/one": true, "/source/two": true})

        area.beginDrag("/source/one", Qt.NoModifier, "", Qt.point(34, 28))

        var dragSource = findChild(area, "fileDragSource")
        verify(dragSource !== null)
        compare(dragSource.Drag.active, true)
        compare(dragSource.Drag.keys.join(","), "text/uri-list,filem/paths")
        compare(area.dragPaths.length, 2)
        compare(area.dragMimeData["filem/paths"], "/source/one\n/source/two")
        compare(dragSource.x + dragSource.width / 2, 34)
        compare(dragSource.y + dragSource.height / 2, 28)
        area.finishDrag()
    }

    function test_draggingAnUnselectedItemUsesOnlyThatItem() {
        area.selection = ({"/source/one": true, "/source/two": true})

        area.beginDrag("/source/other", Qt.NoModifier, "", Qt.point(14, 12))

        compare(area.dragPaths.length, 1)
        compare(area.dragPaths[0], "/source/other")
        area.finishDrag()
    }

    function test_listAndColumnRowsDragFromInteriorAndKeepMarqueeAtEdges() {
        verify(!area.isMarqueeStart(320, 18, 640, 36))
        verify(!area.isMarqueeStart(500, 18, 640, 36))
        verify(area.isMarqueeStart(2, 18, 640, 36))
        verify(area.isMarqueeStart(320, 2, 640, 36))
    }

}
