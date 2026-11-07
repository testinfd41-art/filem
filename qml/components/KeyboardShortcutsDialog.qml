import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    anchors.fill: parent
    visible: open
    z: 452
    focus: open

    property var theme
    property bool open: false

    readonly property var sections: [
        {
            title: "Navigation and windows",
            shortcuts: [
                { keys: "Ctrl+L", action: "Edit the location" },
                { keys: "Ctrl+F", action: "Search files" },
                { keys: "Alt+Left", action: "Go back" },
                { keys: "Alt+Right", action: "Go forward" },
                { keys: "Alt+Up", action: "Open the parent folder" },
                { keys: "Alt+1", action: "Focus the sidebar" },
                { keys: "Alt+2", action: "Focus the file list" },
                { keys: "Ctrl+B", action: "Show or hide the sidebar" },
                { keys: "Ctrl+Alt+B", action: "Show or hide the tab bar" },
                { keys: "Ctrl+T", action: "Open a new tab" },
                { keys: "Ctrl+W", action: "Close the current tab" },
                { keys: "Ctrl+Shift+T", action: "Reopen the last closed tab" },
                { keys: "Ctrl+Tab", action: "Switch to the next tab" },
                { keys: "Ctrl+Shift+Tab", action: "Switch to the previous tab" },
                { keys: "Ctrl+N", action: "Open a new window" },
                { keys: "Ctrl+Q", action: "Quit Filem" }
            ]
        },
        {
            title: "Files and selection",
            shortcuts: [
                { keys: "Ctrl+Shift+N", action: "Create a folder" },
                { keys: "Ctrl+C", action: "Copy the selection" },
                { keys: "Ctrl+X", action: "Cut the selection" },
                { keys: "Ctrl+V", action: "Paste" },
                { keys: "Ctrl+A", action: "Select all items" },
                { keys: "Ctrl+Enter", action: "Open the selection in a new tab" },
                { keys: "F2", action: "Rename the selection" },
                { keys: "Delete", action: "Move the selection to Trash" },
                { keys: "Shift+Delete", action: "Permanently delete the selection" },
                { keys: "Alt+Enter", action: "Show properties" },
                { keys: "Ctrl+Z", action: "Undo the last file operation" },
                { keys: "Ctrl+Shift+Z", action: "Redo the last file operation" },
                { keys: "Ctrl+D", action: "Bookmark the current folder" },
                { keys: "Ctrl+H", action: "Show or hide hidden files" },
                { keys: "Ctrl+R / F5 / Alt+F5", action: "Refresh the current view" }
            ]
        },
        {
            title: "View and appearance",
            shortcuts: [
                { keys: "Ctrl+P", action: "Show or hide the preview pane" },
                { keys: "Space", action: "Open or close Quick Look" },
                { keys: "Ctrl+=", action: "Zoom in" },
                { keys: "Ctrl+Plus", action: "Zoom in (keypad)" },
                { keys: "Ctrl+-", action: "Zoom out" },
                { keys: "Ctrl+Minus", action: "Zoom out (keypad)" },
                { keys: "Ctrl+0", action: "Reset zoom" },
                { keys: "Ctrl+Shift+D", action: "Toggle dark or light mode" }
            ]
        },
        {
            title: "Tags",
            shortcuts: [
                { keys: "T, 1", action: "Apply tag 1 to the selection" },
                { keys: "T, 2", action: "Apply tag 2 to the selection" },
                { keys: "T, 3", action: "Apply tag 3 to the selection" },
                { keys: "T, 4", action: "Apply tag 4 to the selection" },
                { keys: "T, 5", action: "Apply tag 5 to the selection" },
                { keys: "T, 6", action: "Apply tag 6 to the selection" },
                { keys: "T, 7", action: "Apply tag 7 to the selection" }
            ]
        },
        {
            title: "In the file list and menus",
            shortcuts: [
                { keys: "Arrow keys", action: "Move through items; Shift extends selection" },
                { keys: "Ctrl+Arrow", action: "Move without replacing the selection" },
                { keys: "Home / End", action: "Jump to the first or last item" },
                { keys: "PageUp / PageDown", action: "Move by one page" },
                { keys: "Enter", action: "Open an item, run a search, accept a location, or choose a menu item" },
                { keys: "Down", action: "Move from the search field to its results" },
                { keys: "Backspace", action: "Go to the parent folder" },
                { keys: "Type letters", action: "Search by item name in the file list" },
                { keys: "Up / Down", action: "Move through an open menu or dialog list" },
                { keys: "Escape", action: "Close a menu or dialog, cancel, or clear selection" }
            ]
        },
        {
            title: "Quick Look",
            shortcuts: [
                { keys: "Left / Right", action: "Show the previous or next file" },
                { keys: "PageUp / PageDown", action: "Show the previous or next PDF page" },
                { keys: "Space / Escape", action: "Close Quick Look" }
            ]
        }
    ]

    function close() {
        open = false
    }

    function hasShortcut(keys) {
        for (var sectionIndex = 0; sectionIndex < sections.length; sectionIndex++) {
            var shortcuts = sections[sectionIndex].shortcuts
            for (var shortcutIndex = 0; shortcutIndex < shortcuts.length; shortcutIndex++) {
                if (shortcuts[shortcutIndex].keys === keys)
                    return true
            }
        }
        return false
    }

    onOpenChanged: {
        if (open)
            forceActiveFocus()
    }

    Keys.priority: Keys.BeforeItem
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
        objectName: "keyboardShortcutsCard"
        width: Math.min(760, root.width - 40)
        height: Math.min(720, root.height - 40)
        anchors.centerIn: parent
        radius: root.theme ? Math.max(8, root.theme.radius) : 10
        color: root.theme ? root.theme.surface : "#151820"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        MouseArea {
            anchors.fill: parent
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            RowLayout {
                Layout.fillWidth: true
                Layout.preferredHeight: 76
                Layout.leftMargin: 22
                Layout.rightMargin: 18
                spacing: 14

                Rectangle {
                    Layout.preferredWidth: 38
                    Layout.preferredHeight: 38
                    Layout.alignment: Qt.AlignVCenter
                    radius: 9
                    color: root.theme ? root.theme.selection : "#24364f"

                    Canvas {
                        anchors.fill: parent
                        anchors.margins: 8
                        property color ink: root.theme ? root.theme.accent : "#8ab4f8"
                        onInkChanged: requestPaint()
                        Component.onCompleted: requestPaint()
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.strokeStyle = ink
                            ctx.lineWidth = 1.6
                            ctx.lineJoin = "round"
                            ctx.strokeRect(1, 3, width - 2, height - 6)
                            for (var row = 0; row < 2; row++) {
                                for (var column = 0; column < 4; column++) {
                                    var x = 4 + column * 5
                                    var y = 7 + row * 6
                                    ctx.strokeRect(x, y, 2.5, 2.5)
                                }
                            }
                            ctx.beginPath()
                            ctx.moveTo(6, height - 6)
                            ctx.lineTo(width - 6, height - 6)
                            ctx.stroke()
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignVCenter
                    spacing: 3

                    Text {
                        Layout.fillWidth: true
                        text: "Keyboard shortcuts"
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 17 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }

                    Text {
                        Layout.fillWidth: true
                        text: "Browse Filem’s shortcuts by task"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 11 * (root.theme ? root.theme.font_scale : 1)
                    }
                }

                Rectangle {
                    objectName: "closeKeyboardShortcutsButton"
                    Layout.preferredWidth: 34
                    Layout.preferredHeight: 34
                    Layout.alignment: Qt.AlignVCenter
                    radius: root.theme ? Math.max(3, root.theme.radius * 0.45) : 5
                    color: closeMouse.containsMouse
                           ? (root.theme ? root.theme.hover : "#303746")
                           : (root.theme ? root.theme.surface_alt : "#1c202a")
                    border.width: 1
                    border.color: root.theme ? root.theme.border : "#454b58"
                    Accessible.role: Accessible.Button
                    Accessible.name: "Close keyboard shortcuts"

                    Text {
                        anchors.centerIn: parent
                        text: "\u00d7"
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 16 * (root.theme ? root.theme.font_scale : 1)
                    }

                    MouseArea {
                        id: closeMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.close()
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: root.theme ? root.theme.border : "#292e3a"
            }

            ScrollView {
                id: shortcutsScroll
                objectName: "keyboardShortcutsScroll"
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.leftMargin: 22
                Layout.rightMargin: 18
                Layout.topMargin: 14
                Layout.bottomMargin: 10
                clip: true
                contentWidth: availableWidth
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                ColumnLayout {
                    width: shortcutsScroll.availableWidth
                    spacing: 18

                    Repeater {
                        model: root.sections

                        delegate: ColumnLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: 5

                            Text {
                                Layout.fillWidth: true
                                text: modelData.title
                                color: root.theme ? root.theme.accent : "#8ab4f8"
                                font.family: root.theme ? root.theme.font_family : "sans-serif"
                                font.pixelSize: 11 * (root.theme ? root.theme.font_scale : 1)
                                font.weight: Font.DemiBold
                                font.capitalization: Font.AllUppercase
                            }

                            Repeater {
                                model: modelData.shortcuts

                                delegate: Item {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    implicitHeight: Math.max(32, shortcutAction.implicitHeight + 10)

                                    RowLayout {
                                        anchors.fill: parent
                                        spacing: 14

                                        Text {
                                            id: shortcutAction
                                            Layout.fillWidth: true
                                            Layout.alignment: Qt.AlignVCenter
                                            text: modelData.action
                                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                                            font.family: root.theme ? root.theme.font_family : "sans-serif"
                                            font.pixelSize: 11.5 * (root.theme ? root.theme.font_scale : 1)
                                            wrapMode: Text.WordWrap
                                        }

                                        Rectangle {
                                            Layout.alignment: Qt.AlignVCenter
                                            implicitWidth: shortcutKeys.implicitWidth + 18
                                            Layout.minimumWidth: implicitWidth
                                            height: 24
                                            radius: 5
                                            color: root.theme ? root.theme.surface_alt : "#1c202a"
                                            border.width: 1
                                            border.color: root.theme ? root.theme.border : "#292e3a"

                                            Text {
                                                id: shortcutKeys
                                                anchors.centerIn: parent
                                                text: modelData.keys
                                                color: root.theme ? root.theme.muted : "#9299a8"
                                                font.family: root.theme
                                                           ? (root.theme.mono_font || root.theme.font_family)
                                                           : "monospace"
                                                font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: root.theme ? root.theme.border : "#292e3a"
            }

            Text {
                Layout.fillWidth: true
                Layout.leftMargin: 22
                Layout.rightMargin: 22
                Layout.topMargin: 10
                Layout.bottomMargin: 12
                text: "Some keys are context-sensitive. Text fields keep their normal editing shortcuts."
                color: root.theme ? root.theme.muted : "#9299a8"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                wrapMode: Text.WordWrap
            }
        }
    }
}
