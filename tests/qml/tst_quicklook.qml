import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    id: test
    name: "QuickLookNavigation"
    when: windowShown
    width: 900
    height: 640

    QtObject {
        id: fakeBackend
        property var preview_data: ({ title: "Preview" })
        property int item_count: 3
        property string requestedPath: ""
        signal preview_changed()

        function path_at(index) {
            return ["/tmp/first.txt", "/tmp/second.txt", "/tmp/third.txt"][index] || ""
        }

        function request_preview(path) {
            requestedPath = path
            preview_changed()
        }

        function file_url(path) {
            return "file://" + path
        }
    }

    QuickLook {
        id: quickLook
        backend: fakeBackend
    }

    function init() {
        quickLook.close()
        fakeBackend.requestedPath = ""
    }

    function test_arrow_keys_navigate_adjacent_files() {
        quickLook.openFor(fakeBackend.path_at(1), [], 1)
        wait(30)
        compare(quickLook.opened, true)
        compare(fakeBackend.requestedPath, "/tmp/second.txt")

        keyClick(Qt.Key_Right)
        wait(20)
        compare(quickLook.previewPath, "/tmp/third.txt")
        compare(fakeBackend.requestedPath, "/tmp/third.txt")

        keyClick(Qt.Key_Left)
        wait(20)
        compare(quickLook.previewPath, "/tmp/second.txt")
    }

    function test_navigation_stops_at_collection_edges() {
        quickLook.openFor(fakeBackend.path_at(0), [], 0)
        wait(30)

        keyClick(Qt.Key_Left)
        wait(20)
        compare(quickLook.previewPath, "/tmp/first.txt")
        keyClick(Qt.Key_Right)
        wait(20)
        keyClick(Qt.Key_Right)
        wait(20)
        compare(quickLook.previewPath, "/tmp/third.txt")
        keyClick(Qt.Key_Right)
        wait(20)
        compare(quickLook.previewPath, "/tmp/third.txt")
    }
}
