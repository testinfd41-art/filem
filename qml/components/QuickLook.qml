import QtQuick
import QtQuick.Controls
import QtMultimedia

Popup {
    id: root

    property var theme
    property var backend
    property string previewPath: ""
    property var imageSequence: []
    property int imageIndex: 0
    property int navigationIndex: -1
    property bool modelNavigation: false
    property real documentZoom: 1
    property int pendingPdfPage: 0
    property int lastPdfRenderDimension: 0
    readonly property var preview: backend ? backend.preview_data : ({})
    readonly property int documentPageCount: pdfFile && preview ? Number(preview.page_count || 0) : 0
    readonly property int documentPageNumber: pdfFile && preview ? Number(preview.page_number || 1) : 1
    readonly property int documentIndex: pdfFile
                                         ? (pendingPdfPage > 0 ? pendingPdfPage : documentPageNumber)
                                         : (modelNavigation ? navigationIndex + 1 : imageIndex + 1)
    readonly property int documentCount: pdfFile
                                         ? documentPageCount
                                         : (modelNavigation && backend
                                            ? backend.item_count : imageSequence.length)
    readonly property int desiredPdfRenderDimension: Math.min(
        2400,
        Math.ceil(Math.max(
            512,
            Math.max(documentView.width, documentView.height)
                * documentZoom * (Screen.devicePixelRatio || 1) * 2
        ) / 256) * 256
    )
    readonly property bool pdfFile: previewPath.split(".").pop().toLowerCase() === "pdf"
    readonly property bool imageFile: [
        "png", "jpg", "jpeg", "webp", "gif", "svg", "bmp", "tif", "tiff", "avif", "ico"
    ].indexOf(previewPath.split(".").pop().toLowerCase()) >= 0
    readonly property bool videoFile: [
        "mp4", "mkv", "webm", "avi", "mov", "m4v", "mpg", "mpeg", "ogv"
    ].indexOf(previewPath.split(".").pop().toLowerCase()) >= 0
    readonly property bool audioFile: [
        "mp3", "flac", "ogg", "oga", "opus", "m4a", "m4b", "wav", "wave",
        "aac", "wma", "aif", "aiff", "ape", "wv", "3ga"
    ].indexOf(previewPath.split(".").pop().toLowerCase()) >= 0
    readonly property bool markdownFile: ["md", "markdown"].indexOf(
        previewPath.split(".").pop().toLowerCase()) >= 0

    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    anchors.centerIn: Overlay.overlay
    width: Math.min(1100, parent ? parent.width * 0.88 : 900)
    height: Math.min(820, parent ? parent.height * 0.88 : 680)
    padding: 0

    signal dismissed()

    onOpened: forceActiveFocus()

    function openFor(path, sequence, itemIndex) {
        if (!backend || !path)
            return
        previewPath = path
        documentZoom = 1
        imageSequence = sequence && sequence.indexOf(path) >= 0 ? sequence.slice() : []
        imageIndex = Math.max(0, imageSequence.indexOf(path))
        navigationIndex = itemIndex === undefined ? -1 : Number(itemIndex)
        modelNavigation = navigationIndex >= 0
        pendingPdfPage = 0
        lastPdfRenderDimension = 0
        player.stop()
        player.source = ""
        open()
        if (pdfFile) {
            Qt.callLater(function() {
                if (root.opened)
                    root.requestPdfPage(1)
            })
        } else {
            backend.request_preview(path)
        }
        if (videoFile || audioFile) {
            Qt.callLater(function() {
                if (root.opened && (root.videoFile || root.audioFile)) {
                    player.source = backend.file_url(path)
                    if (root.videoFile)
                        player.play()
                }
            })
        }
    }

    onClosed: {
        player.stop()
        player.source = ""
        pendingPdfPage = 0
        lastPdfRenderDimension = 0
        if (backend)
            backend.request_preview("")
        dismissed()
    }

    function navigateDocument(delta) {
        if (documentCount <= 1)
            return
        var next = documentIndex + delta
        if (next < 1 || next > documentCount)
            return
        if (pdfFile) {
            requestPdfPage(next)
        } else {
            navigateAdjacentFile(delta)
        }
    }

    function canNavigateAdjacent() {
        return modelNavigation
               ? backend && backend.item_count > 1
               : imageSequence.length > 1
    }

    function navigateAdjacentFile(delta) {
        if (!backend || !canNavigateAdjacent())
            return
        var nextIndex = modelNavigation ? navigationIndex + delta : imageIndex + delta
        var count = modelNavigation ? backend.item_count : imageSequence.length
        if (nextIndex < 0 || nextIndex >= count)
            return

        var nextPath = modelNavigation
                       ? backend.path_at(nextIndex).toString()
                       : imageSequence[nextIndex]
        if (!nextPath || nextPath === previewPath)
            return

        if (modelNavigation)
            navigationIndex = nextIndex
        else
            imageIndex = nextIndex
        previewPath = nextPath
        documentZoom = 1
        pendingPdfPage = 0
        lastPdfRenderDimension = 0
        player.stop()
        player.source = ""

        if (pdfFile) {
            requestPdfPage(1)
        } else {
            backend.request_preview(nextPath)
            if (videoFile || audioFile) {
                Qt.callLater(function() {
                    if (root.opened && root.previewPath === nextPath) {
                        player.source = backend.file_url(nextPath)
                        if (root.videoFile)
                            player.play()
                    }
                })
            }
        }
    }

    function requestPdfPage(page) {
        if (!backend)
            return
        pendingPdfPage = page
        lastPdfRenderDimension = desiredPdfRenderDimension
        backend.request_preview_page(previewPath, page, desiredPdfRenderDimension)
    }

    function refreshPdfResolution() {
        if (opened && pdfFile && documentPageCount > 0
                && desiredPdfRenderDimension !== lastPdfRenderDimension)
            requestPdfPage(documentIndex)
    }

    function zoomDocument(delta) {
        documentZoom = Math.max(1, Math.min(4, documentZoom + delta))
        pdfResizeTimer.restart()
    }

    Shortcut {
        sequence: "Left"
        context: Qt.WindowShortcut
        enabled: root.opened && root.canNavigateAdjacent()
        onActivated: root.navigateAdjacentFile(-1)
    }

    Connections {
        target: root.backend
        function onPreview_changed() {
            if (!root.pdfFile || !root.preview)
                return
            if (Number(root.preview.page_number || 0) === root.pendingPdfPage
                    || (root.pendingPdfPage > 0 && root.preview.note)) {
                root.pendingPdfPage = 0
            }
        }
    }

    Timer {
        id: pdfResizeTimer
        interval: 180
        repeat: false
        onTriggered: root.refreshPdfResolution()
    }

    Shortcut {
        sequence: "Right"
        context: Qt.WindowShortcut
        enabled: root.opened && root.canNavigateAdjacent()
        onActivated: root.navigateAdjacentFile(1)
    }

    Shortcut {
        sequence: "PageUp"
        context: Qt.WindowShortcut
        enabled: root.opened && root.pdfFile && root.documentIndex > 1
        onActivated: root.navigateDocument(-1)
    }

    Shortcut {
        sequence: "PageDown"
        context: Qt.WindowShortcut
        enabled: root.opened && root.pdfFile && root.documentIndex < root.documentCount
        onActivated: root.navigateDocument(1)
    }

    function formatTime(milliseconds) {
        var seconds = Math.floor(milliseconds / 1000)
        var minutes = Math.floor(seconds / 60)
        return minutes + ":" + ("0" + (seconds % 60)).slice(-2)
    }

    MediaPlayer {
        id: player
        audioOutput: AudioOutput {}
        videoOutput: videoSurface
    }

    background: Rectangle {
        radius: theme ? Math.max(12, theme.radius) : 14
        color: theme ? theme.surface : "#151820"
        border.width: 1
        border.color: theme ? theme.border : "#303746"
    }

    Column {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            width: parent.width
            height: 48
            color: "transparent"

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 18
                anchors.right: documentZoomControls.left
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                text: root.preview && root.preview.title ? root.preview.title : root.previewPath.split("/").pop()
                color: theme ? theme.foreground : "#e7e9ee"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 14 * (theme ? theme.font_scale : 1)
                font.bold: true
                elide: Text.ElideMiddle
            }

            Row {
                id: documentZoomControls
                anchors.right: closeButton.left
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                visible: root.imageFile || root.pdfFile

                ActionButton {
                    theme: root.theme
                    text: "\u2212"
                    font.pixelSize: 18 * (theme ? theme.font_scale : 1)
                    implicitWidth: 32
                    implicitHeight: 32
                    flat: true
                    enabled: root.documentZoom > 1
                    onClicked: root.zoomDocument(-0.25)
                }

                ActionButton {
                    theme: root.theme
                    text: Math.round(root.documentZoom * 100) + "%"
                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                    implicitWidth: 52
                    implicitHeight: 32
                    flat: true
                    onClicked: {
                        root.documentZoom = 1
                        pdfResizeTimer.restart()
                    }
                }

                ActionButton {
                    theme: root.theme
                    text: "+"
                    font.pixelSize: 18 * (theme ? theme.font_scale : 1)
                    implicitWidth: 32
                    implicitHeight: 32
                    flat: true
                    enabled: root.documentZoom < 4
                    onClicked: root.zoomDocument(0.25)
                }
            }

            Text {
                id: closeButton
                anchors.right: parent.right
                anchors.rightMargin: 16
                anchors.verticalCenter: parent.verticalCenter
                text: "\u00d7"
                color: theme ? theme.muted : "#9299a8"
                font.pixelSize: 24 * (theme ? theme.font_scale : 1)
                Accessible.name: "Close Quick Look"

                MouseArea {
                    anchors.fill: parent
                    anchors.margins: -8
                    onClicked: root.close()
                }
            }
        }

        Rectangle {
            width: parent.width
            height: 1
            color: theme ? theme.border : "#303746"
        }

        Item {
            width: parent.width
            height: parent.height - 49

            Flickable {
                id: documentView
                anchors.fill: parent
                anchors.margins: 20
                visible: root.imageFile || root.pdfFile
                clip: true
                interactive: root.documentZoom > 1
                boundsBehavior: Flickable.StopAtBounds
                contentWidth: Math.max(width, documentCanvas.width)
                contentHeight: Math.max(height, documentCanvas.height)
                onWidthChanged: pdfResizeTimer.restart()
                onHeightChanged: pdfResizeTimer.restart()

                Item {
                    id: documentCanvas
                    width: Math.max(documentView.width, documentImage.width)
                    height: Math.max(documentView.height, documentImage.height)

                    Image {
                        id: documentImage
                        anchors.centerIn: parent
                        property real fitScale: {
                            if (implicitWidth <= 0 || implicitHeight <= 0)
                                return 1
                            return Math.min(documentView.width / implicitWidth,
                                            documentView.height / implicitHeight)
                        }
                        width: implicitWidth * fitScale * root.documentZoom
                        height: implicitHeight * fitScale * root.documentZoom
                        source: root.pdfFile
                                ? (root.preview && root.preview.image_url ? root.preview.image_url : "")
                                : (root.imageSequence.length > 0
                                   ? (root.backend
                                      ? root.backend.file_url(root.imageSequence[root.imageIndex])
                                      : "")
                                   : (root.preview && root.preview.image_url
                                      ? root.preview.image_url : ""))
                        sourceSize.width: Math.max(1, documentView.width * root.documentZoom * 2)
                        sourceSize.height: Math.max(1, documentView.height * root.documentZoom * 2)
                        fillMode: Image.PreserveAspectFit
                        asynchronous: true
                        cache: true
                    }
                }

                Text {
                    anchors.centerIn: parent
                    width: documentView.width - 100
                    horizontalAlignment: Text.AlignHCenter
                    visible: documentImage.status !== Image.Ready
                    text: documentImage.status === Image.Error
                          ? (root.pdfFile
                             ? "Could not display this PDF page."
                             : "Could not display this image.")
                          : root.preview && root.preview.note
                            ? root.preview.note : "Loading preview\u2026"
                    color: theme ? theme.muted : "#9299a8"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                    wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                }

                ActionButton {
                    theme: root.theme
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    width: 36
                    height: 48
                    visible: root.documentCount > 1
                    enabled: root.documentIndex > 1
                    text: "\u2039"
                    font.pixelSize: 24 * (theme ? theme.font_scale : 1)
                    flat: true
                    onClicked: root.navigateDocument(-1)
                }

                ActionButton {
                    theme: root.theme
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    width: 36
                    height: 48
                    visible: root.documentCount > 1
                    enabled: root.documentIndex < root.documentCount
                    text: "\u203a"
                    font.pixelSize: 24 * (theme ? theme.font_scale : 1)
                    flat: true
                    onClicked: root.navigateDocument(1)
                }

                Text {
                    anchors.bottom: parent.bottom
                    anchors.horizontalCenter: parent.horizontalCenter
                    height: 30
                    visible: root.documentCount > 1
                    text: root.documentIndex + " / " + root.documentCount
                    color: theme ? theme.muted : "#9299a8"
                    font.family: theme ? theme.mono_font : "monospace"
                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                    verticalAlignment: Text.AlignVCenter
                }

                WheelHandler {
                    acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                    onWheel: function(event) {
                        if (!(event.modifiers & Qt.ControlModifier))
                            return
                        root.zoomDocument(event.angleDelta.y > 0 ? 0.25 : -0.25)
                        event.accepted = true
                    }
                }

                ScrollBar.vertical: ScrollBar {
                    active: documentView.contentHeight > documentView.height
                }
                ScrollBar.horizontal: ScrollBar {
                    active: documentView.contentWidth > documentView.width
                }
            }

            Flickable {
                visible: !root.imageFile && !root.pdfFile
                anchors.fill: parent
                anchors.margins: 20
                clip: true
                contentWidth: width
                contentHeight: previewColumn.implicitHeight
                boundsBehavior: Flickable.StopAtBounds

                Column {
                    id: previewColumn
                    width: parent.width
                    spacing: 16

                    Item {
                        width: parent.width
                        height: Math.max(220, root.height - 210)
                        visible: root.videoFile

                        VideoOutput {
                            id: videoSurface
                            anchors.fill: parent
                            fillMode: VideoOutput.PreserveAspectFit
                        }

                        Text {
                            anchors.centerIn: parent
                            visible: player.error !== MediaPlayer.NoError
                                     || ((player.mediaStatus === MediaPlayer.LoadedMedia
                                          || player.mediaStatus === MediaPlayer.BufferedMedia)
                                         && !player.hasVideo)
                            text: player.error !== MediaPlayer.NoError
                                  ? (player.errorString || "Unable to play this video.")
                                  : "This file has no playable video stream."
                            color: theme ? theme.muted : "#9299a8"
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                            wrapMode: Text.Wrap
                        }
                    }

                    Row {
                        width: parent.width
                        spacing: 10
                        visible: root.audioFile

                        ActionButton {
                            theme: root.theme
                            emphasized: true
                            text: player.playbackState === MediaPlayer.PlayingState ? "Pause" : "Play"
                            enabled: player.mediaStatus !== MediaPlayer.NoMedia
                            onClicked: player.playbackState === MediaPlayer.PlayingState
                                       ? player.pause()
                                       : player.play()
                        }

                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: root.formatTime(player.position)
                            color: theme ? theme.muted : "#9299a8"
                            font.family: theme ? theme.mono_font : "monospace"
                            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                        }

                        Slider {
                            width: Math.max(80, parent.width - 190)
                            anchors.verticalCenter: parent.verticalCenter
                            from: 0
                            to: Math.max(1, player.duration)
                            value: player.position
                            enabled: player.duration > 0
                            onMoved: player.setPosition(value)
                        }

                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: root.formatTime(player.duration)
                            color: theme ? theme.muted : "#9299a8"
                            font.family: theme ? theme.mono_font : "monospace"
                            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                        }
                    }

                    Row {
                        width: parent.width
                        spacing: 10
                        visible: root.videoFile

                        ActionButton {
                            theme: root.theme
                            emphasized: true
                            text: player.playbackState === MediaPlayer.PlayingState ? "Pause" : "Play"
                            onClicked: player.playbackState === MediaPlayer.PlayingState
                                       ? player.pause()
                                       : player.play()
                        }

                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: root.formatTime(player.position)
                            color: theme ? theme.muted : "#9299a8"
                            font.family: theme ? theme.mono_font : "monospace"
                            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                        }

                        Slider {
                            width: Math.max(80, parent.width - 190)
                            anchors.verticalCenter: parent.verticalCenter
                            from: 0
                            to: Math.max(1, player.duration)
                            value: player.position
                            enabled: player.duration > 0
                            onMoved: player.setPosition(value)
                        }

                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: root.formatTime(player.duration)
                            color: theme ? theme.muted : "#9299a8"
                            font.family: theme ? theme.mono_font : "monospace"
                            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                        }
                    }

                    Text {
                        width: parent.width
                        visible: !!(root.preview && root.preview.text)
                        text: root.preview && root.preview.text ? root.preview.text : ""
                        color: theme ? theme.foreground : "#e7e9ee"
                        font.family: root.markdownFile
                                     ? (theme ? theme.font_family : "sans-serif")
                                     : (theme ? theme.mono_font : "monospace")
                        font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                        textFormat: root.markdownFile ? Text.MarkdownText : Text.PlainText
                    }

                    Text {
                        width: parent.width
                        visible: !root.preview || (!root.preview.image_url && !root.preview.text)
                        text: root.preview && root.preview.note
                              ? root.preview.note
                              : "Loading preview\u2026"
                        color: theme ? theme.muted : "#9299a8"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                    }

                    Text {
                        width: parent.width
                        visible: !root.videoFile && !root.pdfFile
                                 && !!(root.preview && root.preview.image_url)
                        text: root.preview && root.preview.note ? root.preview.note : ""
                        color: theme ? theme.muted : "#9299a8"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                    }
                }
            }
        }
    }
}
