import QtQuick

Rectangle {
    id: root

    property var theme
    property var backend
    property bool searchMode: false
    property alias text: input.text

    signal accepted(string path)
    signal canceled()
    signal moveToList()

    implicitHeight: 30
    radius: theme ? Math.max(4, theme.radius * 0.55) : 6
    color: theme ? theme.surface_alt : "#1c202a"
    border.width: 1
    border.color: input.activeFocus
                  ? (theme ? theme.focus : "#8ab4f8")
                  : (theme ? theme.border : "#292e3a")

    Behavior on border.color {
        ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
    }

    function focusInput() {
        input.forceActiveFocus()
        input.selectAll()
    }

    function runSearch() {
        if (!backend)
            return
        if (input.text.trim() === "")
            backend.clear_search()
        else
            backend.search(input.text, backend.search_recursive)
    }

    ToolbarGlyph {
        id: modeIcon
        anchors.left: parent.left
        anchors.leftMargin: 10
        anchors.verticalCenter: parent.verticalCenter
        name: root.searchMode ? "search" : "location"
        color: root.searchMode
               ? (theme ? theme.accent : "#8ab4f8")
               : (theme ? theme.muted : "#9299a8")
    }

    TextInput {
        id: input
        anchors.fill: parent
        anchors.leftMargin: 36
        anchors.rightMargin: root.searchMode ? 120 : 10
        verticalAlignment: TextInput.AlignVCenter
        color: theme ? theme.foreground : "#e7e9ee"
        selectionColor: theme ? theme.selection : "#24364f"
        selectedTextColor: theme ? theme.selection_text : "#f2f5fb"
        font.family: theme ? theme.font_family : "sans-serif"
        font.pixelSize: 13 * (theme ? theme.font_scale : 1)
        selectByMouse: true
        clip: true

        onAccepted: {
            debounce.stop()
            root.accepted(text)
        }
        onTextChanged: {
            if (root.searchMode) {
                if (text.trim() === "") {
                    debounce.stop()
                    if (root.backend)
                        root.backend.clear_search()
                } else {
                    debounce.restart()
                }
            }
        }

        Keys.onEscapePressed: function(event) {
            root.canceled()
            event.accepted = true
        }

        Keys.onDownPressed: function(event) {
            if (root.searchMode)
                root.moveToList()
            event.accepted = root.searchMode
        }

        Text {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: input.text === ""
            text: root.searchMode
                  ? (root.backend && root.backend.recent_view
                     ? "Search recent files: name, ext:pdf, type:image, tag:red\u2026"
                     : "Search this folder: name, ext:pdf, type:image, tag:red\u2026")
                  : "Enter a file or folder location"
            color: theme ? theme.muted : "#9299a8"
            opacity: 0.75
            font: input.font
            elide: Text.ElideRight
        }
    }

    Item {
        visible: root.searchMode && !(root.backend && root.backend.recent_view)
        anchors.right: parent.right
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        width: 106
        height: 26

        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: root.backend && root.backend.search_recursive
                   ? Qt.rgba(theme ? theme.accent.r : 0.54,
                             theme ? theme.accent.g : 0.71,
                             theme ? theme.accent.b : 0.98, 0.13)
                   : (theme ? theme.surface : "#151820")
            border.width: 1
            border.color: root.backend && root.backend.search_recursive
                          ? Qt.rgba(theme ? theme.accent.r : 0.54,
                                    theme ? theme.accent.g : 0.71,
                                    theme ? theme.accent.b : 0.98, 0.55)
                          : (theme ? theme.border : "#292e3a")

            Behavior on color {
                ColorAnimation { duration: 140; easing.type: Easing.OutCubic }
            }
            Behavior on border.color {
                ColorAnimation { duration: 140; easing.type: Easing.OutCubic }
            }

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                text: "Subfolders"
                color: theme ? theme.foreground : "#e7e9ee"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
            }

            Rectangle {
                id: switchTrack
                anchors.right: parent.right
                anchors.rightMargin: 7
                anchors.verticalCenter: parent.verticalCenter
                width: 25
                height: 14
                radius: height / 2
                color: root.backend && root.backend.search_recursive
                       ? (theme ? theme.accent : "#8ab4f8")
                       : (theme ? theme.border : "#454b58")

                Behavior on color {
                    ColorAnimation { duration: 140; easing.type: Easing.OutCubic }
                }

                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    x: root.backend && root.backend.search_recursive ? parent.width - width - 2 : 2
                    width: 10
                    height: 10
                    radius: height / 2
                    color: theme ? theme.surface : "#151820"

                    Behavior on x {
                        NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
                    }
                }
            }

            MouseArea {
                id: scopeMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                Accessible.role: Accessible.Switch
                Accessible.name: "Search subfolders"
                Accessible.checked: root.backend ? root.backend.search_recursive : false
                onClicked: {
                    if (root.backend)
                        root.backend.set_search_recursive(!root.backend.search_recursive)
                }
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
