import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    id: test
    name: "ModalDialogKeyboard"
    when: windowShown
    width: 640
    height: 420

    property bool confirmed: false

    ModalDialog {
        id: dialog
        anchors.fill: parent
        title: "Delete permanently?"
        confirmText: "Delete Permanently"
        destructive: true
        onConfirmed: test.confirmed = true
    }

    function init() {
        confirmed = false
        dialog.open = false
    }

    function test_enterConfirmsDefaultAction() {
        dialog.open = true
        wait(30)

        keyClick(Qt.Key_Return)

        verify(test.confirmed)
        compare(dialog.open, false)
    }

    function test_numpadEnterConfirmsDefaultAction() {
        dialog.open = true
        wait(30)

        keyClick(Qt.Key_Enter)

        verify(test.confirmed)
        compare(dialog.open, false)
    }
}
