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

    title: metadata && metadata.name ? metadata.name + " \u2014 Properties" : "Properties"
    modal: true
    anchors.centerIn: parent
    width: Math.min(780, parent ? parent.width - 64 : 780)
    height: Math.min(690, parent ? parent.height - 64 : 690)
    padding: 18
    standardButtons: Dialog.Close
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    background: Rectangle {
        color: root.theme ? root.theme.surface : "#151820"
        border.color: root.theme ? root.theme.border : "#292e3a"
        border.width: 1
        radius: root.theme ? Math.max(8, root.theme.radius) : 10
    }

    onClosed: {
        if (root.backend && root.backend.properties_open)
            root.backend.close_properties()
    }

    contentItem: ColumnLayout {
        spacing: 12

        Text {
            Layout.fillWidth: true
            text: root.metadata && root.metadata.kind
                  ? root.metadata.kind + (root.metadata.selectedCount > 1
                                          ? " \u00b7 " + root.metadata.selectedCount + " items" : "")
                  : ""
            color: root.theme ? root.theme.muted : "#9299a8"
            font.family: root.theme ? root.theme.font_family : "sans-serif"
            font.pixelSize: 12 * (theme ? theme.font_scale : 1)
            elide: Text.ElideRight
        }

        TabBar {
            id: tabs
            Layout.fillWidth: true
            background: Rectangle { color: "transparent" }

            Repeater {
                model: ["General", "Details", "Metadata", "Permissions", "Location"]
                delegate: TabButton {
                    id: tabButton
                    required property string modelData
                    text: modelData
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                    padding: 10

                    background: Rectangle {
                        radius: root.theme ? Math.max(5, root.theme.radius * 0.5) : 6
                        color: tabButton.checked
                               ? (root.theme ? root.theme.selection : "#24364f")
                               : (tabButton.hovered
                                  ? (root.theme ? root.theme.hover : "#202531")
                                  : "transparent")
                        border.width: tabButton.activeFocus ? 1 : 0
                        border.color: root.theme ? root.theme.focus : "#8ab4f8"

                        Behavior on color {
                            ColorAnimation { duration: 110; easing.type: Easing.OutCubic }
                        }
                    }

                    contentItem: Text {
                        text: tabButton.text
                        color: tabButton.checked
                               ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                               : (root.theme ? root.theme.muted : "#9299a8")
                        font: tabButton.font
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                }
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: tabs.currentIndex

            Repeater {
                model: ["General", "Details", "Metadata", "Permissions", "Location"]
                delegate: Item {
                    id: page
                    required property string modelData
                    readonly property string sectionName: modelData

                    Flickable {
                        anchors.fill: parent
                        clip: true
                        contentWidth: width
                        contentHeight: contentColumn.implicitHeight
                        boundsBehavior: Flickable.StopAtBounds

                        Column {
                            id: contentColumn
                            width: parent.width
                            spacing: 10

                            Repeater {
                                model: root.fields.filter(function(field) {
                                    if (page.sectionName === "Details")
                                        return ["General", "Metadata", "Permissions", "Location"].indexOf(field.section) < 0
                                    if (page.sectionName === "Metadata")
                                        return ["Metadata", "Embedded metadata", "Document metadata", "Document statistics"].indexOf(field.section) >= 0
                                    return field.section === page.sectionName
                                })
                                delegate: RowLayout {
                                    id: row
                                    required property var modelData
                                    width: contentColumn.width
                                    spacing: 14

                                    Text {
                                        Layout.preferredWidth: 170
                                        Layout.alignment: Qt.AlignTop
                                        text: row.modelData.label
                                        color: root.theme ? root.theme.muted : "#9299a8"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                                        wrapMode: Text.Wrap
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        Layout.alignment: Qt.AlignTop
                                        text: row.modelData.value
                                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                                        wrapMode: Text.WrapAnywhere
                                        textFormat: Text.PlainText
                                    }
                                }
                            }

                            Repeater {
                                model: page.sectionName === "Metadata" && root.metadata && root.metadata.warnings
                                       ? root.metadata.warnings : []
                                delegate: Text {
                                    required property string modelData
                                    width: contentColumn.width
                                    text: modelData
                                    color: root.theme ? root.theme.danger : "#e06c75"
                                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
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
