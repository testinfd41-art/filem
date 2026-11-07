import QtQuick

Rectangle {
    id: control

    property var theme
    property string label: ""
    property string tooltip: ""
    property bool active: false
    property bool backgroundless: false

    signal clicked()

    implicitWidth: 34
    implicitHeight: 34
    activeFocusOnTab: true
    scale: mouse.containsMouse && control.enabled ? 1.035 : 1
    radius: theme ? Math.max(7, theme.radius * 0.72) : 8
    color: control.backgroundless
           ? "transparent"
           : (!control.enabled
           ? "transparent"
           : control.active
             ? (theme ? theme.selection : "#24364f")
             : (mouse.containsMouse ? (theme ? theme.hover : "#202531") : "transparent"))
    opacity: control.enabled ? 1.0 : 0.42
    border.width: control.activeFocus ? 1 : 0
    border.color: theme ? theme.focus : "transparent"

    Behavior on color {
        ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
    }

    Behavior on scale {
        NumberAnimation { duration: 100; easing.type: Easing.OutCubic }
    }

    Text {
        anchors.centerIn: parent
        text: control.label
        color: control.active
               ? (theme ? theme.selection_text : "#f2f5fb")
               : (theme ? theme.foreground : "#e7e9ee")
        font.family: theme ? theme.font_family : "sans-serif"
        font.pixelSize: 16 * (theme ? theme.font_scale : 1)
        font.weight: Font.Medium
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        enabled: control.enabled
        onClicked: control.clicked()
    }

    Keys.onPressed: function(event) {
        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
            control.clicked()
            event.accepted = true
        }
    }

    Accessible.role: Accessible.Button
    Accessible.name: control.tooltip !== "" ? control.tooltip : control.label
    Accessible.description: control.tooltip
}
