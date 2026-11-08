import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    id: test
    name: "ImageConvertDialog"
    when: windowShown
    width: 600
    height: 500

    ImageConvertDialog {
        id: dialog
        formats: ["avif", "heic", "heif", "jpeg", "png", "webp"]
    }

    SignalSpy {
        id: chosenSpy
        target: dialog
        signalName: "chosen"
    }

    function init() {
        dialog.open = false
        dialog.filterText = ""
        dialog.selectedFormat = ""
        chosenSpy.clear()
    }

    function test_search_filters_available_formats() {
        dialog.open = true
        dialog.filterText = "hei"
        compare(dialog.formatList.count, 2)
        compare(dialog.formatList.model[0], "heic")
        compare(dialog.formatList.model[1], "heif")
    }

    function test_selecting_format_emits_choice() {
        dialog.open = true
        dialog.selectedFormat = "heic"
        dialog.choose()
        compare(chosenSpy.count, 1)
        compare(chosenSpy.signalArguments[0][0], "heic")
        compare(dialog.open, false)
    }
}
