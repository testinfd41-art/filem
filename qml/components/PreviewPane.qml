import QtQuick
import QtQuick.Controls

Rectangle {
    id: root

    property var theme
    property var backend
    property var fileArea
    property bool shown: false
    readonly property var preview: backend ? backend.preview_data : ({})
    readonly property var metadata: backend ? backend.properties_data : ({})
    readonly property bool metadataReady: fileArea && metadata.path === fileArea.currentPath

    color: theme ? theme.surface : "#151820"
    border.width: 1
    border.color: theme ? theme.border : "#292e3a"
    clip: true

    ToolbarGlyph {
        anchors.left: parent.left
        anchors.leftMargin: 16
        anchors.top: parent.top
        anchors.topMargin: 12
        name: "preview"
        color: theme ? theme.accent : "#8ab4f8"
    }

    Text {
        anchors.left: parent.left
        anchors.leftMargin: 42
        anchors.top: parent.top
        anchors.topMargin: 13
        text: "Preview & Details"
        color: theme ? theme.foreground : "#e7e9ee"
        font.family: theme ? theme.font_family : "sans-serif"
        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
        font.weight: Font.DemiBold
    }

    IconButton {
        visible: root.backend && root.backend.properties_open
        anchors.right: parent.right
        anchors.rightMargin: 8
        anchors.top: parent.top
        anchors.topMargin: 6
        theme: root.theme
        label: "\u00d7"
        tooltip: "Close details pane"
        onClicked: root.backend.close_properties()
    }

    function safeText(value, fallback) {
        return value !== undefined && value !== null && value !== "" ? value : fallback
    }

    function baseTitle() {
        return safeText(preview && preview.title, "")
    }

    Timer {
        id: previewTimer
        interval: 100
        repeat: false
        onTriggered: {
            if (root.shown && root.backend && root.fileArea) {
                root.backend.request_preview(root.fileArea.currentPath)
                root.backend.request_properties(root.fileArea.currentPath)
            }
        }
    }

    Connections {
        target: root.fileArea
        function onCurrentPathChanged() {
            if (root.shown)
                previewTimer.restart()
        }
    }

    onShownChanged: {
        if (shown)
            previewTimer.restart()
        else if (backend)
            backend.request_preview("")
    }

    Component.onCompleted: {
        if (shown)
            previewTimer.restart()
    }

    Flickable {
        id: flick
        anchors.fill: parent
        anchors.margins: 16
        anchors.topMargin: 44
        clip: true
        contentWidth: width
        contentHeight: column.height + 12

        Column {
            id: column
            width: parent.width
            spacing: 14

            Text {
                width: parent.width
                text: root.baseTitle()
                color: theme ? theme.foreground : "#e7e9ee"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 15 * (theme ? theme.font_scale : 1)
                font.bold: true
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
            }

            Text {
                width: parent.width
                text: root.safeText(preview && preview.kind, "")
                color: theme ? theme.accent : "#8ab4f8"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 11 * (theme ? theme.font_scale : 1)
            }

            Text {
                width: parent.width
                text: root.safeText(preview && preview.subtitle, "")
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
            }

            Rectangle {
                width: parent.width
                height: 1
                color: theme ? theme.border : "#292e3a"
            }

            Text {
                width: parent.width
                text: "File details"
                color: theme ? theme.foreground : "#e7e9ee"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
            }

            Text {
                width: parent.width
                visible: !root.metadataReady
                text: "Loading metadata\u2026"
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
            }

            Repeater {
                model: [
                    { label: "Location", value: root.metadataReady ? root.metadata.path || "" : "" },
                    { label: "Type", value: root.metadataReady ? root.metadata.kind || "" : "" },
                    { label: "Size", value: root.metadataReady ? root.metadata.size || "" : "" },
                    { label: "Items", value: root.metadataReady ? root.metadata.items || "" : "" },
                    { label: "Modified", value: root.metadataReady && root.metadata.modified ? new Date(Number(root.metadata.modified)).toLocaleString() : "" },
                    { label: "Accessed", value: root.metadataReady && root.metadata.accessed ? new Date(Number(root.metadata.accessed)).toLocaleString() : "" },
                    { label: "Permissions", value: root.metadataReady ? root.metadata.mode || "" : "" },
                    { label: "Owner / Group", value: root.metadataReady ? root.metadata.owner || "" : "" },
                    { label: "Device ID", value: root.metadataReady ? root.metadata.device || "" : "" },
                    { label: "Inode", value: root.metadataReady ? root.metadata.inode || "" : "" },
                    { label: "Tag", value: root.metadataReady ? root.metadata.tagName || "None" : "" },
                    { label: "Link target", value: root.metadataReady ? root.metadata.linkTarget || "" : "" }
                ]

                delegate: Column {
                    required property var modelData
                    width: column.width
                    spacing: 2
                    visible: modelData.value !== ""

                    Text {
                        width: parent.width
                        text: modelData.label
                        color: theme ? theme.muted : "#9299a8"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 9 * (theme ? theme.font_scale : 1)
                    }
                    Text {
                        width: parent.width
                        text: modelData.value
                        color: theme ? theme.foreground : "#e7e9ee"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                    }
                }
            }

            Rectangle {
                width: parent.width
                height: 1
                color: theme ? theme.border : "#292e3a"
            }

            Image {
                width: parent.width
                height: preview && preview.image_url ? Math.min(220, Math.max(120, parent.width * 0.7)) : 0
                visible: !!(preview && preview.image_url)
                source: preview && preview.image_url ? preview.image_url : ""
                fillMode: Image.PreserveAspectFit
                sourceSize.width: width
                sourceSize.height: height
                asynchronous: true
            }

            Rectangle {
                visible: !!(preview && preview.note)
                width: parent.width
                height: noteText.contentHeight + 18
                radius: theme ? Math.max(5, theme.radius * 0.5) : 6
                color: theme ? theme.surface_alt : "#10141b"
                border.width: 1
                border.color: theme ? theme.border : "#292e3a"

                Text {
                    id: noteText
                    anchors.fill: parent
                    anchors.margins: 10
                    text: root.safeText(preview && preview.note, "")
                    color: theme ? theme.muted : "#9299a8"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                    wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                }
            }

            Text {
                width: parent.width
                visible: !!(preview && preview.text)
                text: root.safeText(preview && preview.text, "")
                color: theme ? theme.foreground : "#e7e9ee"
                font.family: theme ? theme.mono_font : "monospace"
                font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
            }

            Column {
                width: parent.width
                visible: !!(preview && preview.archive && preview.archive.length)
                spacing: 4

                Repeater {
                    model: preview && preview.archive ? preview.archive : []
                    delegate: Text {
                        width: parent.width
                        text: modelData || ""
                        color: theme ? theme.muted : "#9299a8"
                        font.family: theme ? theme.mono_font : "monospace"
                        font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                        elide: Text.ElideRight
                    }
                }
            }
        }
    }
}
