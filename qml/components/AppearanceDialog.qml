import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    anchors.fill: parent
    visible: open
    z: 450
    focus: open

    property var theme
    property bool open: false
    property string errorMessage: ""

    function saveSetting(key, value) {
        if (!theme || !theme.set_appearance(key, String(value))) {
            errorMessage = "Could not save this appearance setting."
            return false
        }
        errorMessage = ""
        return true
    }

    function saveColor(key, value) {
        if (!/^#[0-9a-fA-F]{6}$/.test(value)) {
            errorMessage = "Enter a color in #RRGGBB format."
            return false
        }
        return saveSetting(key, value)
    }

    function colorValue(key) {
        if (!theme)
            return "#000000"
        switch (key) {
        case "background": return theme.background
        case "surface": return theme.surface
        case "surfaceAlt": return theme.surface_alt
        case "foreground": return theme.foreground
        case "muted": return theme.muted
        case "accent": return theme.accent
        case "border": return theme.border
        case "selection": return theme.selection
        case "selectionText": return theme.selection_text
        case "hover": return theme.hover
        case "focus": return theme.focus
        case "danger": return theme.danger
        case "tag1": return theme.tag1
        case "tag2": return theme.tag2
        case "tag3": return theme.tag3
        case "tag4": return theme.tag4
        case "tag5": return theme.tag5
        case "tag6": return theme.tag6
        case "tag7": return theme.tag7
        default: return "#000000"
        }
    }

    function close() {
        open = false
    }

    onOpenChanged: {
        if (open) {
            errorMessage = ""
            forceActiveFocus()
        }
    }

    Keys.onEscapePressed: function(event) {
        close()
        event.accepted = true
    }

    Rectangle {
        anchors.fill: parent
        color: "#000000"
        opacity: 0.55
        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }
    }

    Rectangle {
        id: card
        width: Math.min(700, root.width - 40)
        height: Math.min(680, root.height - 40)
        anchors.centerIn: parent
        radius: root.theme ? Math.max(6, root.theme.radius) : 10
        color: root.theme ? root.theme.surface : "#151820"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 14

            RowLayout {
                Layout.fillWidth: true
                Text {
                    Layout.fillWidth: true
                    text: "Appearance"
                    color: root.theme ? root.theme.foreground : "#e7e9ee"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 17 * (root.theme ? root.theme.font_scale : 1)
                    font.weight: Font.DemiBold
                }
                Rectangle {
                    width: 32
                    height: 32
                    radius: 6
                    color: root.theme ? root.theme.surface_alt : "#1c202a"
                    Text {
                        anchors.centerIn: parent
                        text: "×"
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        font.pixelSize: 18 * (root.theme ? root.theme.font_scale : 1)
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: root.close()
                    }
                    Accessible.name: "Close appearance settings"
                    Accessible.role: Accessible.Button
                }
            }

            ScrollView {
                id: scroll
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                ScrollBar.vertical: ScrollBar {
                    policy: ScrollBar.AsNeeded
                    width: 10
                }

                ColumnLayout {
                    width: Math.max(0, scroll.availableWidth - 14)
                    spacing: 12

                    Text {
                        text: "Interface"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }

                    GridLayout {
                        Layout.fillWidth: true
                        columns: 2
                        columnSpacing: 12
                        rowSpacing: 8

                        Text {
                            text: "Font family"
                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                            font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                        }
                        ComboBox {
                            id: fontControl
                            objectName: "appearanceFont"
                            Accessible.name: "Font family"
                            Layout.fillWidth: true
                            editable: true
                            model: Qt.fontFamilies()
                            currentIndex: root.theme ? model.indexOf(root.theme.font_family) : -1
                            onActivated: root.saveSetting("fontFamily", currentText)
                            onAccepted: root.saveSetting("fontFamily", editText)
                            contentItem: TextField {
                                text: fontControl.editable ? fontControl.editText : fontControl.displayText
                                color: root.theme ? root.theme.foreground : "#e7e9ee"
                                selectionColor: root.theme ? root.theme.selection : "#24364f"
                                selectedTextColor: root.theme ? root.theme.selection_text : "#f2f5fb"
                                font.family: root.theme ? root.theme.font_family : "sans-serif"
                                font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                                verticalAlignment: TextInput.AlignVCenter
                                leftPadding: 8
                                rightPadding: fontControl.indicator.width + 8
                                background: null
                                onTextEdited: fontControl.editText = text
                                onAccepted: root.saveSetting("fontFamily", text)
                            }
                            background: Rectangle {
                                radius: 5
                                color: root.theme ? root.theme.surface_alt : "#1c202a"
                                border.width: 1
                                border.color: fontControl.activeFocus
                                              ? (root.theme ? root.theme.focus : "#8ab4f8")
                                              : (root.theme ? root.theme.border : "#292e3a")
                            }
                            delegate: ItemDelegate {
                                width: fontControl.width
                                contentItem: Text {
                                    text: modelData
                                    rightPadding: 14
                                    color: root.theme ? root.theme.foreground : "#e7e9ee"
                                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                                    font.pixelSize: (Math.round(12 * (root.theme ? root.theme.font_scale : 1)) | 0)
                                    verticalAlignment: Text.AlignVCenter
                                }
                                background: Rectangle {
                                    color: highlighted
                                           ? (root.theme ? root.theme.hover : "#202531")
                                           : (root.theme ? root.theme.surface_alt : "#1c202a")
                                }
                            }
                            popup: Popup {
                                y: fontControl.height - 1
                                width: fontControl.width
                                implicitHeight: contentItem.implicitHeight
                                padding: 1
                                contentItem: ListView {
                                    clip: true
                                    implicitHeight: Math.min(contentHeight, 280)
                                    model: fontControl.popup.visible ? fontControl.delegateModel : null
                                    currentIndex: fontControl.highlightedIndex
                                    boundsBehavior: Flickable.StopAtBounds
                                    ScrollBar.vertical: ScrollBar {
                                        policy: ScrollBar.AsNeeded
                                        width: 8
                                    }
                                }
                                background: Rectangle {
                                    color: root.theme ? root.theme.surface_alt : "#1c202a"
                                    border.color: root.theme ? root.theme.border : "#292e3a"
                                    radius: 5
                                }
                            }
                        }

                        Text {
                            text: "Font size"
                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                            font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                        }
                        SpinBox {
                            id: sizeControl
                            objectName: "appearanceFontSize"
                            Accessible.name: "Font size"
                            Layout.fillWidth: true
                            editable: true
                            from: 9
                            to: 24
                            value: root.theme ? Math.round(root.theme.font_size) : 12
                            onValueModified: root.saveSetting("fontSize", value)
                            contentItem: TextInput {
                                text: sizeControl.displayText
                                color: root.theme ? root.theme.foreground : "#e7e9ee"
                                selectionColor: root.theme ? root.theme.selection : "#24364f"
                                selectedTextColor: root.theme ? root.theme.selection_text : "#f2f5fb"
                                font.family: root.theme ? root.theme.font_family : "sans-serif"
                                font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                                horizontalAlignment: TextInput.AlignHCenter
                                verticalAlignment: TextInput.AlignVCenter
                                readOnly: false
                                validator: sizeControl.validator
                                inputMethodHints: Qt.ImhFormattedNumbersOnly
                            }
                            background: Rectangle {
                                radius: 5
                                color: root.theme ? root.theme.surface_alt : "#1c202a"
                                border.width: 1
                                border.color: sizeControl.activeFocus
                                              ? (root.theme ? root.theme.focus : "#8ab4f8")
                                              : (root.theme ? root.theme.border : "#292e3a")
                            }
                        }

                    }

                    Text {
                        Layout.topMargin: 8
                        text: "Interface colors"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }

                    Repeater {
                        model: [
                            { key: "background", label: "Window background" },
                            { key: "surface", label: "Surface" },
                            { key: "surfaceAlt", label: "Alternate surface" },
                            { key: "foreground", label: "Text" },
                            { key: "muted", label: "Muted text" },
                            { key: "accent", label: "Accent" },
                            { key: "border", label: "Border" },
                            { key: "selection", label: "Selection" },
                            { key: "selectionText", label: "Selection text" },
                            { key: "hover", label: "Hover" },
                            { key: "focus", label: "Focus" },
                            { key: "danger", label: "Danger" }
                        ]
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            Text {
                                Layout.fillWidth: true
                                text: modelData.label
                                color: root.theme ? root.theme.foreground : "#e7e9ee"
                                font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                            }
                            Rectangle {
                                Layout.preferredWidth: 20
                                Layout.preferredHeight: 20
                                radius: 4
                                color: root.colorValue(modelData.key)
                                border.width: 1
                                border.color: root.theme ? root.theme.border : "#292e3a"
                            }
                            TextField {
                                objectName: "color_" + modelData.key
                                Layout.preferredWidth: 110
                                text: root.colorValue(modelData.key)
                                selectByMouse: true
                                color: root.theme ? root.theme.foreground : "#e7e9ee"
                                selectionColor: root.theme ? root.theme.selection : "#24364f"
                                selectedTextColor: root.theme ? root.theme.selection_text : "#f2f5fb"
                                font.family: root.theme ? root.theme.font_family : "sans-serif"
                                font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                                Accessible.name: modelData.label + " color"
                                onEditingFinished: root.saveColor(modelData.key, text)
                                background: Rectangle {
                                    radius: 5
                                    color: root.theme ? root.theme.surface_alt : "#1c202a"
                                    border.width: 1
                                    border.color: parent.activeFocus
                                                  ? (root.theme ? root.theme.focus : "#8ab4f8")
                                                  : (root.theme ? root.theme.border : "#292e3a")
                                }
                            }
                        }
                    }

                    Text {
                        Layout.topMargin: 8
                        text: "Tag colors"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }

                    Repeater {
                        model: [
                            { key: "tag1", label: "Tag 1" },
                            { key: "tag2", label: "Tag 2" },
                            { key: "tag3", label: "Tag 3" },
                            { key: "tag4", label: "Tag 4" },
                            { key: "tag5", label: "Tag 5" },
                            { key: "tag6", label: "Tag 6" },
                            { key: "tag7", label: "Tag 7" }
                        ]
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            Text {
                                Layout.fillWidth: true
                                text: modelData.label
                                color: root.theme ? root.theme.foreground : "#e7e9ee"
                                font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                            }
                            Rectangle {
                                Layout.preferredWidth: 20
                                Layout.preferredHeight: 20
                                radius: 4
                                color: root.colorValue(modelData.key)
                                border.width: 1
                                border.color: root.theme ? root.theme.border : "#292e3a"
                            }
                            TextField {
                                objectName: "color_" + modelData.key
                                Layout.preferredWidth: 110
                                text: root.colorValue(modelData.key)
                                selectByMouse: true
                                color: root.theme ? root.theme.foreground : "#e7e9ee"
                                selectionColor: root.theme ? root.theme.selection : "#24364f"
                                selectedTextColor: root.theme ? root.theme.selection_text : "#f2f5fb"
                                font.family: root.theme ? root.theme.font_family : "sans-serif"
                                font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                                Accessible.name: modelData.label + " color"
                                onEditingFinished: root.saveColor(modelData.key, text)
                                background: Rectangle {
                                    radius: 5
                                    color: root.theme ? root.theme.surface_alt : "#1c202a"
                                    border.width: 1
                                    border.color: parent.activeFocus
                                                  ? (root.theme ? root.theme.focus : "#8ab4f8")
                                                  : (root.theme ? root.theme.border : "#292e3a")
                                }
                            }
                        }
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                visible: root.errorMessage !== ""
                text: root.errorMessage
                color: root.theme ? root.theme.danger : "#f28b82"
                font.pixelSize: 11 * (root.theme ? root.theme.font_scale : 1)
            }
        }
    }
}
