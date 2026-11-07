pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root

    property var theme
    property var backend

    readonly property var metadata: backend ? backend.properties_data : ({})
    readonly property var fields: metadata && metadata.details ? metadata.details : []
    readonly property int selectedCount: metadata && metadata.selectedCount !== undefined
                                         ? metadata.selectedCount : 1
    readonly property bool multiple: selectedCount > 1
    readonly property string itemName: metadata && metadata.name ? metadata.name : "Properties"
    readonly property string subtitle: {
        var parts = []
        if (metadata && metadata.kind)
            parts.push(metadata.kind)
        if (!multiple && metadata && metadata.size)
            parts.push(metadata.size)
        return parts.join(" \u00b7 ")
    }

    readonly property var sections: ["General", "Details", "Metadata", "Permissions", "Location"]
    readonly property real uiScale: theme ? theme.font_scale : 1

    property int currentSection: 0

    function fieldsFor(sectionName) {
        var all = root.fields || []
        var result = []
        for (var i = 0; i < all.length; i++) {
            var field = all[i]
            if (sectionName === "Details") {
                if (["General", "Metadata", "Permissions", "Location"].indexOf(field.section) < 0)
                    result.push(field)
            } else if (sectionName === "Metadata") {
                if (["Metadata", "Embedded metadata", "Document metadata", "Document statistics"].indexOf(field.section) >= 0)
                    result.push(field)
            } else if (field.section === sectionName) {
                result.push(field)
            }
        }
        return result
    }

    // Keep fields grouped by their original section so long metadata pages stay
    // readable without changing any of the rows the backend reports.
    function groupedFields(sectionName) {
        var rows = fieldsFor(sectionName)
        var groups = []
        for (var i = 0; i < rows.length; i++) {
            var title = rows[i].section || sectionName
            var group = null
            for (var j = 0; j < groups.length; j++) {
                if (groups[j].title === title) {
                    group = groups[j]
                    break
                }
            }
            if (!group) {
                group = { title: title, fields: [] }
                groups.push(group)
            }
            group.fields.push(rows[i])
        }
        // A single group that just repeats the tab name does not need a heading.
        if (groups.length === 1 && groups[0].title === sectionName)
            groups[0].title = ""
        return groups
    }

    title: itemName + " \u2014 Properties"
    modal: true
    anchors.centerIn: parent
    width: Math.min(680, parent ? parent.width - 64 : 680)
    height: Math.min(520, parent ? parent.height - 64 : 520)
    padding: 0
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    // Suppress the default Controls header band; the custom header lives in the
    // content item so it can match the inspector's surface exactly.
    header: Item {
        implicitHeight: 0
    }

    background: Rectangle {
        color: root.theme ? root.theme.surface : "#151820"
        border.color: root.theme ? root.theme.border : "#292e3a"
        border.width: 1
        radius: root.theme ? Math.max(8, root.theme.radius) : 10
    }

    // Match the dimmed backdrop used by the app's other modal panels.
    Overlay.modal: Rectangle {
        color: "#000000"
        opacity: 0.5
    }

    onClosed: {
        if (root.backend && root.backend.properties_open)
            root.backend.close_properties()
    }

    contentItem: ColumnLayout {
        spacing: 0

        // Header -----------------------------------------------------------------
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 92

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 24
                anchors.rightMargin: 18
                anchors.topMargin: 20
                anchors.bottomMargin: 12
                spacing: 16

                Image {
                    Layout.preferredWidth: 56
                    Layout.preferredHeight: 56
                    Layout.alignment: Qt.AlignVCenter
                    source: root.metadata && root.metadata.iconUrl ? root.metadata.iconUrl : ""
                    sourceSize: Qt.size(112, 112)
                    fillMode: Image.PreserveAspectFit
                    smooth: true
                    visible: source.toString() !== ""
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignVCenter
                    spacing: 3

                    Text {
                        Layout.fillWidth: true
                        text: root.itemName
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 18 * root.uiScale
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.subtitle
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 11.5 * root.uiScale
                        elide: Text.ElideRight
                        visible: text !== ""
                    }
                }

                Rectangle {
                    Layout.preferredWidth: 28
                    Layout.preferredHeight: 28
                    Layout.alignment: Qt.AlignTop
                    radius: 6
                    color: closeMouse.containsMouse
                           ? (root.theme ? root.theme.hover : "#202531") : "transparent"

                    Text {
                        anchors.centerIn: parent
                        text: "\u00d7"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 18 * root.uiScale
                    }

                    MouseArea {
                        id: closeMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.close()
                    }

                    Accessible.name: "Close properties"
                    Accessible.role: Accessible.Button
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: root.theme ? root.theme.border : "#292e3a"
        }

        // Section switcher -------------------------------------------------------
        Item {
            id: sectionSwitcher
            Layout.fillWidth: true
            Layout.preferredHeight: 32
            Layout.leftMargin: 24
            Layout.rightMargin: 24
            Layout.topMargin: 14
            focus: true

            Keys.onLeftPressed: function(event) {
                root.currentSection = Math.max(0, root.currentSection - 1)
                event.accepted = true
            }
            Keys.onRightPressed: function(event) {
                root.currentSection = Math.min(root.sections.length - 1, root.currentSection + 1)
                event.accepted = true
            }

            Rectangle {
                anchors.fill: parent
                radius: 8
                color: root.theme ? root.theme.surface_alt : "#1c202a"
                border.width: 1
                border.color: root.theme ? root.theme.border : "#292e3a"

                Row {
                    id: tabRow
                    anchors.fill: parent
                    anchors.margins: 3
                    spacing: 2

                    Repeater {
                        model: root.sections

                        delegate: Rectangle {
                            id: tabButton
                            required property string modelData
                            required property int index

                            width: (tabRow.width - (root.sections.length - 1) * tabRow.spacing)
                                   / root.sections.length
                            height: tabRow.height
                            radius: 6
                            color: root.currentSection === tabButton.index
                                   ? (root.theme ? root.theme.selection : "#24364f")
                                   : (tabMouse.containsMouse
                                      ? (root.theme ? root.theme.hover : "#202531")
                                      : "transparent")

                            Behavior on color {
                                ColorAnimation { duration: 100; easing.type: Easing.OutCubic }
                            }

                            Text {
                                anchors.centerIn: parent
                                text: tabButton.modelData
                                color: root.currentSection === tabButton.index
                                       ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                                       : (root.theme ? root.theme.muted : "#9299a8")
                                font.family: root.theme ? root.theme.font_family : "sans-serif"
                                font.pixelSize: 11 * root.uiScale
                                font.weight: root.currentSection === tabButton.index
                                             ? Font.DemiBold : Font.Normal
                            }

                            MouseArea {
                                id: tabMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.currentSection = tabButton.index
                            }

                            Accessible.name: tabButton.modelData
                            Accessible.role: Accessible.PageTab
                        }
                    }
                }
            }
        }

        // Pages ------------------------------------------------------------------
        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 24
            Layout.rightMargin: 24
            Layout.topMargin: 10
            Layout.bottomMargin: 4
            currentIndex: root.currentSection

            Repeater {
                model: root.sections

                delegate: Item {
                    id: page
                    required property string modelData

                    Flickable {
                        anchors.fill: parent
                        clip: true
                        contentWidth: width
                        contentHeight: contentColumn.implicitHeight + 10
                        boundsBehavior: Flickable.StopAtBounds

                        ScrollBar.vertical: ScrollBar {
                            policy: ScrollBar.AsNeeded
                            width: 10
                        }

                        Column {
                            id: contentColumn
                            width: Math.max(0, parent.width - 12)
                            y: 4
                            spacing: 18

                            Repeater {
                                model: root.groupedFields(page.modelData)

                                delegate: Column {
                                    id: groupColumn
                                    required property var modelData

                                    width: contentColumn.width
                                    spacing: 0

                                    property int fieldCount: modelData.fields.length

                                    Text {
                                        width: parent.width
                                        text: groupColumn.modelData.title
                                        visible: text !== ""
                                        color: root.theme ? root.theme.muted : "#9299a8"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 10.5 * root.uiScale
                                        font.weight: Font.DemiBold
                                        font.letterSpacing: 0.3
                                        bottomPadding: 5
                                    }

                                    Repeater {
                                        property var groupFields: groupColumn.modelData.fields

                                        model: groupFields

                                        delegate: Item {
                                            id: fieldRow
                                            required property var modelData
                                            required property int index

                                            width: groupColumn.width
                                            height: fieldLayout.implicitHeight + 14

                                            RowLayout {
                                                id: fieldLayout
                                                width: parent.width
                                                anchors.top: parent.top
                                                anchors.topMargin: 7
                                                spacing: 18

                                                Text {
                                                    Layout.preferredWidth: 150
                                                    Layout.alignment: Qt.AlignTop
                                                    text: fieldRow.modelData.label
                                                    color: root.theme ? root.theme.muted : "#9299a8"
                                                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                                                    font.pixelSize: 11 * root.uiScale
                                                    horizontalAlignment: Text.AlignRight
                                                    wrapMode: Text.Wrap
                                                }

                                                Text {
                                                    Layout.fillWidth: true
                                                    Layout.alignment: Qt.AlignTop
                                                    text: fieldRow.modelData.value !== ""
                                                          ? fieldRow.modelData.value : "\u2014"
                                                    color: fieldRow.modelData.value !== ""
                                                           ? (root.theme ? root.theme.foreground : "#e7e9ee")
                                                           : (root.theme ? root.theme.muted : "#9299a8")
                                                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                                                    font.pixelSize: 11 * root.uiScale
                                                    wrapMode: Text.WrapAnywhere
                                                    textFormat: Text.PlainText
                                                }
                                            }

                                            Rectangle {
                                                anchors.left: parent.left
                                                anchors.right: parent.right
                                                anchors.bottom: parent.bottom
                                                height: 1
                                                color: root.theme ? root.theme.border : "#292e3a"
                                                opacity: 0.55
                                                visible: fieldRow.index < groupColumn.fieldCount - 1
                                            }
                                        }
                                    }
                                }
                            }

                            Text {
                                width: contentColumn.width
                                visible: root.fieldsFor(page.modelData).length === 0
                                         && !(page.modelData === "Metadata" && root.metadata
                                              && root.metadata.warnings
                                              && root.metadata.warnings.length > 0)
                                text: "No additional information available."
                                color: root.theme ? root.theme.muted : "#9299a8"
                                font.family: root.theme ? root.theme.font_family : "sans-serif"
                                font.pixelSize: 11.5 * root.uiScale
                                horizontalAlignment: Text.AlignHCenter
                                topPadding: 36
                            }

                            Repeater {
                                model: page.modelData === "Metadata"
                                       && root.metadata && root.metadata.warnings
                                       ? root.metadata.warnings : []

                                delegate: Rectangle {
                                    id: warningBanner
                                    required property string modelData

                                    width: contentColumn.width
                                    height: warningColumn.implicitHeight + 20
                                    radius: 7
                                    color: root.theme ? root.theme.surface_alt : "#1c202a"
                                    border.width: 1
                                    border.color: root.theme ? root.theme.danger : "#e06c75"

                                    Column {
                                        id: warningColumn
                                        anchors.fill: parent
                                        anchors.margins: 10
                                        spacing: 3

                                        Text {
                                            text: "Metadata warning"
                                            color: root.theme ? root.theme.danger : "#e06c75"
                                            font.family: root.theme ? root.theme.font_family : "sans-serif"
                                            font.pixelSize: 10.5 * root.uiScale
                                            font.weight: Font.DemiBold
                                        }

                                        Text {
                                            width: parent.width
                                            text: warningBanner.modelData
                                            color: root.theme ? root.theme.danger : "#e06c75"
                                            font.family: root.theme ? root.theme.font_family : "sans-serif"
                                            font.pixelSize: 11 * root.uiScale
                                            wrapMode: Text.Wrap
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

    // Footer ---------------------------------------------------------------------
    footer: Item {
        implicitHeight: 64

        Rectangle {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            height: 1
            color: root.theme ? root.theme.border : "#292e3a"
        }

        Text {
            anchors.left: parent.left
            anchors.leftMargin: 24
            anchors.verticalCenter: parent.verticalCenter
            text: root.multiple ? root.selectedCount + " items selected" : ""
            color: root.theme ? root.theme.muted : "#9299a8"
            font.family: root.theme ? root.theme.font_family : "sans-serif"
            font.pixelSize: 10.5 * root.uiScale
            visible: text !== ""
        }

        Rectangle {
            id: doneButton
            anchors.right: parent.right
            anchors.rightMargin: 24
            anchors.verticalCenter: parent.verticalCenter
            width: doneLabel.implicitWidth + 34
            height: 32
            radius: 7
            color: doneMouse.containsMouse
                   ? (root.theme ? root.theme.hover : "#202531")
                   : (root.theme ? root.theme.selection : "#24364f")
            border.width: 1
            border.color: root.theme ? root.theme.border : "#292e3a"

            Text {
                id: doneLabel
                anchors.centerIn: parent
                text: "Done"
                color: root.theme ? root.theme.selection_text : "#f2f5fb"
                font.family: root.theme ? root.theme.font_family : "sans-serif"
                font.pixelSize: 12 * root.uiScale
                font.weight: Font.DemiBold
            }

            MouseArea {
                id: doneMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.close()
            }

            Accessible.name: "Close properties"
            Accessible.role: Accessible.Button
        }
    }
}
