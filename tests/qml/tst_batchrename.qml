import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    name: "BatchRenameDialog"
    when: windowShown
    width: 900
    height: 680

    QtObject {
        id: fakeBackend
        property var batch_preview_rows: []
        property string batch_preview_id: ""
        property bool batch_preview_busy: false
        property string batch_preview_error: ""
        property int requestCount: 0
        property real op_progress: 0
        property string op_detail: ""
        signal batch_preview_changed()

        function request_batch_preview(paths, configuration, excludedPaths) {
            requestCount++
            batch_preview_id = String(requestCount)
            batch_preview_busy = true
            var settings = JSON.parse(configuration)
            var rows = []
            for (var i = 0; i < paths.length; i++) {
                var name = paths[i].split("/").pop()
                var excluded = excludedPaths.indexOf(paths[i]) >= 0
                rows.push({
                    path: paths[i],
                    original: name,
                    proposed: settings.template === "" ? name : settings.template,
                    status: excluded ? "Excluded" : "Ready",
                    reason: "",
                    included: !excluded,
                    unchanged: false,
                    conflict: false,
                    oldPrefix: "",
                    oldChanged: name,
                    oldSuffix: "",
                    newPrefix: "",
                    newChanged: settings.template === "" ? name : settings.template,
                    newSuffix: ""
                })
            }
            batch_preview_rows = rows
            batch_preview_busy = false
            batch_preview_error = ""
            batch_preview_changed()
        }

        function clear_batch_preview() {
            batch_preview_rows = []
            batch_preview_id = ""
            batch_preview_changed()
        }

        function execute_batch_rename() {
            var rows = batch_preview_rows.slice()
            for (var i = 0; i < rows.length; i++) {
                var copy = Object.assign({}, rows[i])
                copy.status = copy.included ? "Renamed" : "Excluded"
                rows[i] = copy
            }
            batch_preview_rows = rows
            batch_preview_changed()
            return true
        }

        function cancel_operation() {}
    }

    BatchRenameDialog {
        id: dialog
        backend: fakeBackend
    }

    function test_preview_exclusions_and_confirmed_completion() {
        fakeBackend.requestCount = 0
        dialog.openForPaths(["/tmp/first.jpg", "/tmp/second.jpg"])
        tryCompare(fakeBackend, "requestCount", 1)
        compare(dialog.selectedCount, 2)
        compare(dialog.affectedCount, 2)
        verify(dialog.canExecute)

        dialog.toggleIncluded("/tmp/second.jpg")
        tryCompare(fakeBackend, "requestCount", 2)
        compare(dialog.affectedCount, 1)
        compare(dialog.excludedCount, 1)
        verify(dialog.canExecute)

        dialog.execute()
        verify(dialog.completed)
        compare(fakeBackend.batch_preview_rows[0].status, "Renamed")
        compare(fakeBackend.batch_preview_rows[1].status, "Excluded")
        dialog.close()
    }
}
