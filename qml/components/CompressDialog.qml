import QtQuick

Item {
    id: root

    anchors.fill: parent
    z: 450
    visible: open

    property var theme
    property bool open: false
    property int itemCount: 0
    property string selectedFormat: "zip"
    property bool encrypt: false
    property string password: ""
    property string confirmPassword: ""

    signal chosen(string format, string password)
    signal canceled()

    function resetSecret() {
        encrypt = false
        password = ""
        confirmPassword = ""
    }

    function close() {
        if (!open)
            return
        open = false
        resetSecret()
        canceled()
    }

    function submit() {
        if (encrypt && (password === "" || password !== confirmPassword))
            return
        var format = selectedFormat
        var secret = encrypt ? password : ""
        open = false
        resetSecret()
        chosen(format, secret)
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
        width: Math.min(380, root.width - 48)
        height: content.height + 40
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
                text: "Create Archive"
                color: root.theme ? root.theme.foreground : "#e7e9ee"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 15 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
            }

            Text {
                text: root.itemCount + (root.itemCount === 1 ? " item selected" : " items selected")
                color: root.theme ? root.theme.muted : "#9299a8"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
            }

            Repeater {
                model: [
                    { label: "ZIP archive (.zip)", format: "zip" },
                    { label: "7-Zip archive (.7z)", format: "7z" },
                    { label: "TAR archive (.tar)", format: "tar" },
                    { label: "Compressed TAR archive (.tar.gz)", format: "tar.gz" }
                ]

                delegate: Rectangle {
                    required property var modelData
                    width: content.width
                    height: 38
                    radius: 5
                    color: root.selectedFormat === modelData.format
                           ? (root.theme ? root.theme.selection : "#24364f")
                           : (formatMouse.containsMouse
                              ? (root.theme ? root.theme.hover : "#202531")
                              : (root.theme ? root.theme.surface_alt : "#1c202a"))
                    border.width: 1
                    border.color: root.theme ? root.theme.border : "#292e3a"

                    Text {
                        anchors.fill: parent
                        anchors.leftMargin: 12
                        verticalAlignment: Text.AlignVCenter
                        text: modelData.label
                        color: root.selectedFormat === modelData.format
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
                        onClicked: {
                            root.selectedFormat = modelData.format
                            if (modelData.format !== "zip" && modelData.format !== "7z")
                                root.resetSecret()
                        }
                    }
                }
            }

            Rectangle {
                width: content.width
                height: 34
                visible: root.selectedFormat === "zip" || root.selectedFormat === "7z"
                radius: 5
                color: encryptMouse.containsMouse
                       ? (root.theme ? root.theme.hover : "#202531")
                       : "transparent"

                Text {
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.encrypt ? "\u2611  Encrypt with password" : "\u2610  Encrypt with password"
                    color: root.theme ? root.theme.foreground : "#e7e9ee"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }

                MouseArea {
                    id: encryptMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        root.encrypt = !root.encrypt
                        if (!root.encrypt)
                            root.resetSecret()
                    }
                }
            }

            Column {
                width: content.width
                spacing: 7
                visible: root.encrypt

                Rectangle {
                    width: parent.width
                    height: 34
                    radius: 5
                    color: root.theme ? root.theme.surface_alt : "#1c202a"
                    border.width: 1
                    border.color: root.theme ? root.theme.border : "#292e3a"

                    TextInput {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 10
                        verticalAlignment: TextInput.AlignVCenter
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        selectionColor: root.theme ? root.theme.selection : "#24364f"
                        selectedTextColor: root.theme ? root.theme.selection_text : "#f2f5fb"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                        echoMode: TextInput.Password
                        text: root.password
                        onTextChanged: root.password = text

                        Text {
                            anchors.fill: parent
                            verticalAlignment: Text.AlignVCenter
                            visible: !parent.text
                            text: "Password"
                            color: root.theme ? root.theme.muted : "#9299a8"
                            font: parent.font
                        }
                    }
                }

                Rectangle {
                    width: parent.width
                    height: 34
                    radius: 5
                    color: root.theme ? root.theme.surface_alt : "#1c202a"
                    border.width: 1
                    border.color: root.theme ? root.theme.border : "#292e3a"

                    TextInput {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 10
                        verticalAlignment: TextInput.AlignVCenter
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        selectionColor: root.theme ? root.theme.selection : "#24364f"
                        selectedTextColor: root.theme ? root.theme.selection_text : "#f2f5fb"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                        echoMode: TextInput.Password
                        text: root.confirmPassword
                        onTextChanged: root.confirmPassword = text

                        Text {
                            anchors.fill: parent
                            verticalAlignment: Text.AlignVCenter
                            visible: !parent.text
                            text: "Confirm password"
                            color: root.theme ? root.theme.muted : "#9299a8"
                            font: parent.font
                        }
                    }
                }

                Text {
                    width: parent.width
                    visible: root.password !== "" && root.confirmPassword !== ""
                             && root.password !== root.confirmPassword
                    text: "Passwords do not match."
                    color: root.theme ? root.theme.danger : "#e06c75"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                }
            }

            Text {
                width: content.width
                wrapMode: Text.Wrap
                text: "Password encryption is available for ZIP and 7z only. TAR archives are not encrypted."
                color: root.theme ? root.theme.muted : "#9299a8"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
            }

            Rectangle {
                width: content.width
                height: 32
                radius: 5
                color: createMouse.containsMouse
                       ? (root.theme ? root.theme.hover : "#202531")
                       : (root.theme ? root.theme.selection : "#24364f")
                border.width: 1
                border.color: root.theme ? root.theme.border : "#292e3a"
                opacity: !root.encrypt || (root.password !== "" && root.password === root.confirmPassword)
                         ? 1 : 0.5

                Text {
                    anchors.centerIn: parent
                    text: "Create Archive"
                    color: root.theme ? root.theme.selection_text : "#f2f5fb"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }

                MouseArea {
                    id: createMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    enabled: !root.encrypt
                             || (root.password !== "" && root.password === root.confirmPassword)
                    onClicked: root.submit()
                }
            }

            Rectangle {
                width: content.width
                height: 32
                radius: 5
                color: cancelMouse.containsMouse
                       ? (root.theme ? root.theme.hover : "#202531")
                       : "transparent"
                border.width: 1
                border.color: root.theme ? root.theme.border : "#292e3a"
                Text {
                    anchors.centerIn: parent
                    text: "Cancel"
                    color: root.theme ? root.theme.muted : "#9299a8"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }
                MouseArea {
                    id: cancelMouse
                    anchors.fill: parent
                    hoverEnabled: true
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
