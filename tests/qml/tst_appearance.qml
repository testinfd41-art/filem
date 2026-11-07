import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    name: "AppearanceDialog"
    when: windowShown
    width: 900
    height: 760

    QtObject {
        id: fakeTheme
        signal changed()
        property string mode: "dark"
        property string font_family: "Ubuntu Sans"
        property real font_size: 12
        property real radius: 8
        property string background: "#0f1115"
        property string surface: "#151820"
        property string surface_alt: "#1c202a"
        property string foreground: "#e7e9ee"
        property string muted: "#9299a8"
        property string accent: "#8ab4f8"
        property string border: "#292e3a"
        property string selection: "#24364f"
        property string selection_text: "#f2f5fb"
        property string hover: "#202531"
        property string focus: "#8ab4f8"
        property string danger: "#f28b82"
        property string tag1: "#f28b82"
        property string tag2: "#f5a97f"
        property string tag3: "#e5c07b"
        property string tag4: "#8fbf7f"
        property string tag5: "#8ab4f8"
        property string tag6: "#c39bea"
        property string tag7: "#9aa0ac"
        property string lastKey: ""
        property string lastValue: ""

        function set_appearance(key, value) {
            lastKey = key
            lastValue = value
            return true
        }
    }

    AppearanceDialog {
        id: dialog
        theme: fakeTheme
        open: true
    }

    function test_color_controls_cover_interface_and_all_tags() {
        var keys = [
            "background", "surface", "surfaceAlt", "foreground", "muted",
            "accent", "border", "selection", "selectionText", "hover",
            "focus", "danger", "tag1", "tag2", "tag3", "tag4", "tag5",
            "tag6", "tag7"
        ]
        for (var i = 0; i < keys.length; i++)
            verify(findChild(dialog, "color_" + keys[i]) !== null, keys[i])
    }

    function test_color_validation_and_setting_forwarding() {
        verify(!dialog.saveColor("tag3", "red"))
        compare(fakeTheme.lastKey, "")
        verify(dialog.saveColor("tag3", "#123456"))
        compare(fakeTheme.lastKey, "tag3")
        compare(fakeTheme.lastValue, "#123456")
        verify(dialog.saveSetting("fontSize", 18))
        compare(fakeTheme.lastKey, "fontSize")
        compare(fakeTheme.lastValue, "18")
    }

    function test_tag_color_field_preserves_text_while_editing() {
        var field = findChild(dialog, "color_tag3")
        verify(field !== null)
        field.forceActiveFocus()
        field.text = "#123456"
        fakeTheme.tag3 = "#654321"
        fakeTheme.changed()
        compare(field.text, "#123456")
    }

    function test_primary_controls_are_available() {
        verify(findChild(dialog, "appearanceMode") === null)
        verify(findChild(dialog, "appearanceFont") !== null)
        verify(findChild(dialog, "appearanceFontSize") !== null)
        verify(findChild(dialog, "appearanceOpacity") === null)
    }

    function test_font_picker_is_populated_and_bounded() {
        var fontControl = findChild(dialog, "appearanceFont")
        verify(fontControl.model.length > 0)
        fontControl.popup.open()
        tryCompare(fontControl.popup, "visible", true)
        verify(fontControl.popup.height <= 282)
        fontControl.popup.close()
    }

    function test_escape_closes_appearance_dialog() {
        dialog.open = true
        dialog.forceActiveFocus()
        keyClick(Qt.Key_Escape)
        tryCompare(dialog, "open", false)
    }

}
