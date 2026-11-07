import QtQuick

Item {
    id: root

    anchors.fill: parent
    z: 450
    visible: open
    focus: open

    property var theme
    property bool open: false
    property string fileName: ""
    property var applications: []
    property int currentIndex: -1
    property alias applicationList: appList

    signal chosen(string appId)
    signal canceled()

    function close() {
        if (!open)
            return
        open = false
        canceled()
    }

    function choose(index) {
        if (index < 0 || index >= applications.length)
            return
        var appId = applications[index].id
        open = false
        chosen(appId)
    }

    function moveSelection(delta) {
        if (applications.length === 0)
            return
        currentIndex = Math.max(0, Math.min(applications.length - 1, currentIndex + delta))
        appList.positionViewAtIndex(currentIndex, ListView.Contain)
    }

    onOpenChanged: {
        if (!open)
            return
        currentIndex = applications.length > 0 ? 0 : -1
        Qt.callLater(function() {
            if (root.open) {
                appList.forceActiveFocus()
                if (root.currentIndex >= 0)
                    appList.positionViewAtIndex(root.currentIndex, ListView.Contain)
            }
        })
    }

    onApplicationsChanged: {
        if (!open)
            return
        currentIndex = applications.length > 0 ? 0 : -1
        if (currentIndex >= 0)
            Qt.callLater(function() { appList.positionViewAtIndex(root.currentIndex, ListView.Contain) })
    }

    onCurrentIndexChanged: {
        if (appList.currentIndex !== currentIndex)
            appList.currentIndex = currentIndex
    }

    Keys.onUpPressed: function(event) {
        moveSelection(-1)
        event.accepted = true
    }

    Keys.onDownPressed: function(event) {
        moveSelection(1)
        event.accepted = true
    }

    Keys.onReturnPressed: function(event) {
        choose(currentIndex)
        event.accepted = true
    }

    Keys.onEnterPressed: function(event) {
        choose(currentIndex)
        event.accepted = true
    }

    Keys.onEscapePressed: function(event) {
        close()
        event.accepted = true
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
        width: Math.min(440, root.width - 48)
        height: Math.min(root.height - 64, Math.max(190, contentColumn.height + 40))
        anchors.centerIn: parent
        radius: root.theme ? Math.max(6, root.theme.radius) : 10
        color: root.theme ? root.theme.surface : "#151820"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        Column {
            id: contentColumn
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: 20
            spacing: 12

            Text {
                text: "Open With"
                color: root.theme ? root.theme.foreground : "#e7e9ee"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 15 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
            }

            Text {
                width: parent.width
                text: root.fileName
                elide: Text.ElideMiddle
                color: root.theme ? root.theme.muted : "#9299a8"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
            }

            Rectangle {
                width: parent.width
                height: Math.min(280, Math.max(64, root.applications.length * 38))
                radius: 6
                color: root.theme ? root.theme.surface_alt : "#1c202a"
                border.width: 1
                border.color: root.theme ? root.theme.border : "#292e3a"
                clip: true

                ListView {
                    id: appList
                    anchors.fill: parent
                    model: root.applications
                    currentIndex: root.currentIndex
                    clip: true
                    keyNavigationEnabled: true
                    boundsBehavior: Flickable.StopAtBounds

                    onCurrentIndexChanged: {
                        if (root.currentIndex !== currentIndex)
                            root.currentIndex = currentIndex
                    }

                    Keys.onReturnPressed: function(event) {
                        root.choose(currentIndex)
                        event.accepted = true
                    }

                    Keys.onEnterPressed: function(event) {
                        root.choose(currentIndex)
                        event.accepted = true
                    }

                    delegate: Rectangle {
                        id: applicationRow
                        required property var modelData
                        required property int index
                        width: appList.width
                        height: 36
                        color: index === root.currentIndex || appMouse.containsMouse
                               ? (root.theme ? root.theme.hover : "#202531")
                               : "transparent"
                        border.width: index === root.currentIndex && root.activeFocus ? 1 : 0
                        border.color: root.theme ? root.theme.focus : "#8ab4f8"

                        Text {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 12
                            verticalAlignment: Text.AlignVCenter
                            text: applicationRow.modelData.name
                            elide: Text.ElideRight
                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                            font.family: root.theme ? root.theme.font_family : "sans-serif"
                            font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                        }

                        MouseArea {
                            id: appMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onEntered: root.currentIndex = applicationRow.index
                            onClicked: root.choose(applicationRow.index)
                        }

                        Accessible.role: Accessible.ListItem
                        Accessible.name: modelData.name
                        Accessible.selected: index === root.currentIndex
                    }
                }
            }

            Text {
                visible: root.applications.length === 0
                width: parent.width
                text: "No compatible applications found."
                color: root.theme ? root.theme.muted : "#9299a8"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
            }

            Row {
                width: parent.width
                height: 32
                layoutDirection: Qt.RightToLeft

                ActionButton {
                    width: 84
                    height: 32
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
