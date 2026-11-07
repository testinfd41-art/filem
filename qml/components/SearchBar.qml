import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root

    height: 44
    color: theme ? theme.surface : "#151820"

    property var theme
    property var backend
    property alias text: input.text
    readonly property bool inputFocused: input.activeFocus

    signal canceled()
    signal moveToList()

    function focusInput() {
        input.forceActiveFocus()
        input.selectAll()
    }

    function runSearch() {
        if (!backend)
            return
        if (input.text.trim() === "") {
            backend.clear_search()
            return
        }
        backend.search(input.text, backend.search_recursive)
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: theme ? theme.border : "#292e3a"
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 14
        anchors.rightMargin: 10
        spacing: 10

        ToolbarGlyph {
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: 18
            name: "search"
            color: theme ? theme.muted : "#9299a8"
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.minimumWidth: 150
            Layout.preferredHeight: 30
            Layout.alignment: Qt.AlignVCenter
            radius: theme ? Math.max(6, theme.radius * 0.62) : 7
            color: theme ? theme.surface_alt : "#1c202a"
            border.width: input.activeFocus ? 1 : 0
            border.color: input.activeFocus
                          ? (theme ? theme.focus : "#8ab4f8")
                          : "transparent"

            Behavior on border.color {
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }

            TextInput {
                id: input
                anchors.fill: parent
                anchors.leftMargin: 10
                anchors.rightMargin: 10
                verticalAlignment: TextInput.AlignVCenter
                color: theme ? theme.foreground : "#e7e9ee"
                selectionColor: theme ? theme.selection : "#24364f"
                selectedTextColor: theme ? theme.selection_text : "#f2f5fb"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                selectByMouse: true
                clip: true
                onTextChanged: {
                    if (text === "") {
                        debounce.stop()
                        if (root.backend)
                            root.backend.clear_search()
                    } else {
                        debounce.restart()
                    }
                }
                onAccepted: {
                    debounce.stop()
                    root.runSearch()
                }
                Keys.onEscapePressed: function(event) {
                    debounce.stop()
                    input.text = ""
                    if (root.backend)
                        root.backend.clear_search()
                    root.canceled()
                    event.accepted = true
                }
                Keys.onDownPressed: function(event) {
                    root.moveToList()
                    event.accepted = true
                }

                Text {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    visible: input.text === ""
                    text: "Search this folder — name, ext:pdf, type:image, tag:red, size:>10mb"
                    color: theme ? theme.muted : "#9299a8"
                    opacity: 0.75
                    font: input.font
                    elide: Text.ElideRight
                }
            }
        }

        Text {
            Layout.preferredWidth: 190
            Layout.maximumWidth: 240
            Layout.alignment: Qt.AlignVCenter
            text: root.backend && root.backend.searching
                  ? "Searching\u2026"
                  : (root.backend ? root.backend.search_summary : "")
            elide: Text.ElideRight
            color: theme ? theme.muted : "#9299a8"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
        }

        Rectangle {
            Layout.preferredWidth: 92
            Layout.preferredHeight: 28
            Layout.alignment: Qt.AlignVCenter
            radius: theme ? Math.max(4, theme.radius * 0.5) : 6
            color: root.backend && root.backend.search_recursive
                   ? (theme ? theme.selection : "#24364f")
                   : (scopeMouse.containsMouse
                      ? (theme ? theme.hover : "#202531")
                      : "transparent")
            border.width: 1
            border.color: theme ? theme.border : "#292e3a"

            Text {
                anchors.centerIn: parent
                text: "Subfolders"
                color: root.backend && root.backend.search_recursive
                       ? (theme ? theme.selection_text : "#f2f5fb")
                       : (theme ? theme.muted : "#9299a8")
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 11 * (theme ? theme.font_scale : 1)
            }

            MouseArea {
                id: scopeMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    if (root.backend)
                        root.backend.set_search_recursive(!root.backend.search_recursive)
                }
            }
        }

        IconButton {
            Layout.alignment: Qt.AlignVCenter
            theme: root.theme
            label: "\u00d7"
            tooltip: "Close search (Esc)"
            onClicked: {
                debounce.stop()
                input.text = ""
                if (root.backend)
                    root.backend.clear_search()
                root.canceled()
            }
        }
    }

    Timer {
        id: debounce
        interval: 220
        repeat: false
        onTriggered: root.runSearch()
    }
}
