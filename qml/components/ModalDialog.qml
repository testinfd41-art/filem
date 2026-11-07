import QtQuick

/// One dialog component for confirmations, name prompts and conflict choices.
/// The owning window sets `open`; every dismissal emits `canceled`.
Item {
    id: root

    anchors.fill: parent
    visible: open || opacity > 0
    opacity: open ? 1 : 0
    z: 400

    property var theme
    property bool open: false
    property string title: ""
    property string message: ""
    property string confirmText: "OK"
    property string cancelText: "Cancel"
    property bool destructive: false
    property bool showCancel: true
    property bool showInput: false
    property bool secretInput: false
    property string inputText: ""
    property string placeholder: ""

    /// Extra choices, each { text, value, danger }. They appear before the
    /// confirm button and emit extraChosen(value).
    property var extraButtons: []

    signal confirmed(string text)
    signal canceled()
    signal extraChosen(string value)

    Behavior on opacity {
        NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
    }

    property var buttons: {
        var list = []
        if (showCancel)
            list.push({ label: cancelText, value: "__cancel__", danger: false })
        for (var i = 0; i < extraButtons.length; i++) {
            list.push({
                label: extraButtons[i].text,
                value: extraButtons[i].value,
                danger: extraButtons[i].danger === true
            })
        }
        list.push({ label: confirmText, value: "__confirm__", danger: destructive })
        return list
    }

    onOpenChanged: {
        if (!open)
            return
        input.text = inputText
        if (showInput) {
            Qt.callLater(function() {
                input.forceActiveFocus()
                input.selectAll()
            })
        } else {
            Qt.callLater(function() {
                dialogScope.forceActiveFocus()
            })
        }
    }

    function confirm() {
        var text = showInput ? input.text : ""
        open = false
        if (secretInput) {
            input.clear()
            inputText = ""
        }
        confirmed(text)
    }

    function cancel() {
        if (!open)
            return
        open = false
        if (secretInput) {
            input.clear()
            inputText = ""
        }
        canceled()
    }

    Rectangle {
        anchors.fill: parent
        color: "#000000"
        opacity: 0.5

        MouseArea {
            anchors.fill: parent
            onClicked: root.cancel()
        }
    }

    Rectangle {
        id: card
        width: Math.min(560, root.width - 72)
        height: body.height + 40
        anchors.centerIn: parent
        radius: theme ? Math.max(6, theme.radius) : 10
        color: theme ? theme.surface : "#151820"
        border.width: 1
        border.color: theme ? theme.border : "#292e3a"
        scale: root.open ? 1 : 0.98

        Behavior on scale {
            NumberAnimation { duration: 160; easing.type: Easing.OutCubic }
        }

        FocusScope {
            id: dialogScope
            anchors.fill: parent
            focus: true

            Keys.onEscapePressed: function(event) {
                root.cancel()
                event.accepted = true
            }

            Column {
                id: body
                x: 20
                y: 20
                width: parent.width - 40
                spacing: 12

                Text {
                    width: parent.width
                    text: root.title
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 15 * (theme ? theme.font_scale : 1)
                    font.weight: Font.DemiBold
                    wrapMode: Text.WordWrap
                }

                Text {
                    width: parent.width
                    visible: root.message !== ""
                    text: root.message
                    color: theme ? theme.muted : "#9299a8"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    lineHeight: 1.35
                    wrapMode: Text.WordWrap
                }

                Rectangle {
                    width: parent.width
                    height: root.showInput ? 34 : 0
                    visible: root.showInput
                    radius: theme ? Math.max(4, theme.radius * 0.5) : 6
                    color: theme ? theme.surface_alt : "#1c202a"
                    border.width: 1
                    border.color: input.activeFocus
                                  ? (theme ? theme.focus : "#8ab4f8")
                                  : (theme ? theme.border : "#292e3a")

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
                        echoMode: root.secretInput ? TextInput.Password : TextInput.Normal
                        selectByMouse: true
                        clip: true
                        onAccepted: root.confirm()
                        Keys.onEscapePressed: function(event) {
                            root.cancel()
                            event.accepted = true
                        }
                    }

                    Text {
                        anchors.left: input.left
                        anchors.right: input.right
                        anchors.verticalCenter: input.verticalCenter
                        visible: input.text === ""
                        text: root.placeholder
                        color: theme ? theme.muted : "#9299a8"
                        opacity: 0.7
                        font: input.font
                        elide: Text.ElideRight
                    }
                }

                Row {
                    id: buttonRow
                    width: parent.width
                    height: 32
                    spacing: 8
                    layoutDirection: Qt.RightToLeft

                    Repeater {
                        model: root.buttons

                        delegate: Rectangle {
                            required property var modelData

                            width: Math.max(76, label.implicitWidth + 24)
                            height: 32
                            radius: theme ? Math.max(4, theme.radius * 0.55) : 6
                            color: {
                                if (modelData.danger)
                                    return buttonMouse.containsMouse
                                           ? (theme ? theme.danger : "#e06c75")
                                           : (theme ? theme.surface_alt : "#1c202a")
                                var primary = modelData.value === "__confirm__"
                                if (primary)
                                    return buttonMouse.containsMouse
                                           ? (theme ? theme.accent : "#8ab4f8")
                                           : (theme ? theme.selection : "#24364f")
                                return buttonMouse.containsMouse
                                       ? (theme ? theme.hover : "#202531")
                                       : (theme ? theme.surface_alt : "#1c202a")
                            }
                            border.width: 1
                            border.color: modelData.danger
                                          ? (theme ? theme.danger : "#e06c75")
                                          : (theme ? theme.border : "#292e3a")

                            Text {
                                id: label
                                anchors.centerIn: parent
                                text: modelData.label
                                color: modelData.danger
                                       ? (theme ? theme.danger : "#e06c75")
                                       : (theme ? theme.foreground : "#e7e9ee")
                                font.family: theme ? theme.font_family : "sans-serif"
                                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                            }

                            MouseArea {
                                id: buttonMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    if (modelData.value === "__confirm__") {
                                        root.confirm()
                                    } else if (modelData.value === "__cancel__") {
                                        root.cancel()
                                    } else {
                                        root.open = false
                                        root.extraChosen(modelData.value)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
