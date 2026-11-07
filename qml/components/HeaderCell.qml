import QtQuick

Rectangle {
    id: root

    property var theme
    property string label: ""
    property bool active: false
    property bool ascending: true
    property int alignment: Text.AlignLeft

    signal clicked()

    implicitHeight: 30
    color: "transparent"

    Row {
        anchors.fill: parent
        anchors.leftMargin: 4
        anchors.rightMargin: 4
        spacing: 4
        layoutDirection: root.alignment === Text.AlignRight ? Qt.RightToLeft : Qt.LeftToRight

        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: root.label
            color: root.active
                   ? (theme ? theme.foreground : "#e7e9ee")
                   : (theme ? theme.muted : "#9299a8")
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
            font.weight: root.active ? Font.DemiBold : Font.Normal
        }

        Text {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.active
            text: root.ascending ? "\u2191" : "\u2193"
            color: theme ? theme.accent : "#8ab4f8"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 10 * (theme ? theme.font_scale : 1)
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: root.active ? (theme ? theme.accent : "#8ab4f8") : "transparent"
        opacity: mouse.containsMouse ? 0.75 : 0.35
    }
}
