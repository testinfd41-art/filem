import QtQuick

Item {
    id: root

    anchors.fill: parent
    z: 450
    visible: open
    focus: open

    property var theme
    property bool open: false
    property int itemCount: 0
    property var formats: []
    property string filterText: ""
    property string selectedFormat: ""
    readonly property alias formatList: formatListView

    signal chosen(string format)
    signal canceled()

    function filteredFormats() {
        var query = filterText.trim().toLowerCase()
        if (query === "")
            return formats
        return formats.filter(function(format) {
            return format.toLowerCase().indexOf(query) !== -1
        })
    }

    function close() {
        if (!open)
            return
        open = false
        canceled()
    }

    function choose() {
        if (selectedFormat === "")
            return
        var format = selectedFormat
        open = false
        chosen(format)
    }

    onOpenChanged: {
        if (!open)
            return
        filterText = ""
        selectedFormat = formats.indexOf("png") >= 0
                ? "png" : (formats.length > 0 ? formats[0] : "")
        Qt.callLater(function() {
            if (root.open) {
                formatListView.forceActiveFocus()
                if (formatListView.count > 0)
                    formatListView.positionViewAtIndex(0, ListView.Contain)
            }
        })
    }

    onFilterTextChanged: {
        var matches = filteredFormats()
        if (matches.length === 0)
            selectedFormat = ""
        else if (matches.indexOf(selectedFormat) < 0)
            selectedFormat = matches[0]
    }

    Rectangle {
        anchors.fill: parent
        color: "#000000"
        opacity: 0.5
        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }
    }

    Rectangle {
        id: card
        width: Math.min(460, root.width - 48)
        height: Math.min(root.height - 64, content.height + 40)
        anchors.centerIn: parent
        radius: root.theme ? Math.max(6, root.theme.radius) : 10
        color: root.theme ? root.theme.surface : "#151820"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        Column {
            id: content
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: 20
            spacing: 10

            Text {
                text: "Convert Images"
                color: root.theme ? root.theme.foreground : "#e7e9ee"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 15 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
            }

            Text {
                width: parent.width
                text: root.itemCount + (root.itemCount === 1 ? " image selected" : " images selected")
                color: root.theme ? root.theme.muted : "#9299a8"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
            }

            Rectangle {
                width: parent.width
                height: 34
                radius: 5
                color: root.theme ? root.theme.surface_alt : "#1c202a"
                border.width: 1
                border.color: root.theme ? root.theme.border : "#292e3a"

                TextInput {
                    id: formatSearch
                    anchors.fill: parent
                    anchors.leftMargin: 10
                    anchors.rightMargin: 10
                    verticalAlignment: TextInput.AlignVCenter
                    color: root.theme ? root.theme.foreground : "#e7e9ee"
                    selectionColor: root.theme ? root.theme.selection : "#24364f"
                    selectedTextColor: root.theme ? root.theme.selection_text : "#f2f5fb"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    text: root.filterText
                    onTextChanged: root.filterText = text
                    Keys.onReturnPressed: function(event) {
                        root.choose()
                        event.accepted = true
                    }

                    Text {
                        anchors.fill: parent
                        verticalAlignment: Text.AlignVCenter
                        visible: !formatSearch.text
                        text: "Search available formats"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font: formatSearch.font
                    }
                }
            }

            Rectangle {
                width: parent.width
                height: Math.min(260, Math.max(64, Math.min(formatListView.count * 34, root.height - 320)))
                radius: 6
                color: root.theme ? root.theme.surface_alt : "#1c202a"
                border.width: 1
                border.color: root.theme ? root.theme.border : "#292e3a"
                clip: true

                ListView {
                    id: formatListView
                    anchors.fill: parent
                    model: root.filteredFormats()
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds
                    keyNavigationEnabled: true
                    currentIndex: model.length > 0
                                  ? Math.max(0, model.indexOf(root.selectedFormat)) : -1

                    delegate: Rectangle {
                        required property string modelData
                        required property int index
                        width: formatListView.width
                        height: 34
                        color: modelData === root.selectedFormat || formatMouse.containsMouse
                               ? (root.theme ? root.theme.hover : "#202531")
                               : "transparent"

                        Text {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 12
                            verticalAlignment: Text.AlignVCenter
                            text: modelData.toUpperCase() + "  (." + modelData + ")"
                            color: modelData === root.selectedFormat
                                   ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                                   : (root.theme ? root.theme.foreground : "#e7e9ee")
                            font.family: root.theme ? root.theme.font_family : "sans-serif"
                            font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                        }

                        MouseArea {
                            id: formatMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.selectedFormat = modelData
                        }

                        Accessible.role: Accessible.ListItem
                        Accessible.name: modelData.toUpperCase()
                        Accessible.selected: modelData === root.selectedFormat
                    }

                    Keys.onReturnPressed: function(event) {
                        if (currentIndex >= 0) {
                            root.selectedFormat = model[currentIndex]
                            root.choose()
                        }
                        event.accepted = true
                    }
                    Keys.onEnterPressed: function(event) {
                        if (currentIndex >= 0) {
                            root.selectedFormat = model[currentIndex]
                            root.choose()
                        }
                        event.accepted = true
                    }
                }
            }

            Text {
                width: parent.width
                visible: formatListView.count === 0
                text: "No formats match the search."
                color: root.theme ? root.theme.muted : "#9299a8"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
            }

            Text {
                width: parent.width
                text: "Originals are kept. Some formats may be lossy."
                color: root.theme ? root.theme.muted : "#9299a8"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
            }

            Row {
                width: parent.width
                spacing: 8
                layoutDirection: Qt.RightToLeft

                ActionButton {
                    theme: root.theme
                    text: "Convert"
                    emphasized: true
                    enabled: root.selectedFormat !== ""
                    onClicked: root.choose()
                }

                ActionButton {
                    theme: root.theme
                    text: "Cancel"
                    onClicked: root.close()
                }
            }
        }
    }

    Shortcut {
        sequence: "Escape"
        context: Qt.WindowShortcut
        enabled: root.open
        onActivated: root.close()
    }
}
