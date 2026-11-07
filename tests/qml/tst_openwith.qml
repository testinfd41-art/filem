import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    id: test
    name: "OpenWithDialogKeyboard"
    when: windowShown
    width: 560
    height: 420
    property string chosenId: ""

    OpenWithDialog {
        id: dialog
        anchors.fill: parent
        onChosen: function(appId) { test.chosenId = appId }
    }

    function init() {
        chosenId = ""
        dialog.open = false
        dialog.applications = []
    }

    function test_arrow_keys_select_and_enter_launches_application() {
        dialog.applications = [
            { id: "org.example.First.desktop", name: "First" },
            { id: "org.example.Second.desktop", name: "Second" },
            { id: "org.example.Third.desktop", name: "Third" }
        ]
        dialog.open = true
        wait(30)
        compare(dialog.currentIndex, 0)
        verify(dialog.applicationList.activeFocus)

        keyClick(Qt.Key_Down)
        compare(dialog.currentIndex, 1)
        keyClick(Qt.Key_Return)

        compare(test.chosenId, "org.example.Second.desktop")
        compare(dialog.open, false)
    }

    function test_selection_stays_within_application_list() {
        dialog.applications = [
            { id: "org.example.First.desktop", name: "First" },
            { id: "org.example.Second.desktop", name: "Second" }
        ]
        dialog.open = true
        wait(30)

        keyClick(Qt.Key_Up)
        compare(dialog.currentIndex, 0)
        keyClick(Qt.Key_Down)
        keyClick(Qt.Key_Down)
        compare(dialog.currentIndex, 1)
    }

    function test_empty_application_list_and_escape() {
        dialog.open = true
        wait(30)
        compare(dialog.currentIndex, -1)
        keyClick(Qt.Key_Return)
        compare(dialog.open, true)
        compare(test.chosenId, "")

        keyClick(Qt.Key_Escape)
        compare(dialog.open, false)
    }
}
