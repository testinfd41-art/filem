pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    anchors.fill: parent
    visible: open
    z: 451
    focus: open

    property var theme
    property var lifecycle
    property bool open: false

    readonly property var timeoutSeconds: [5, 10, 30, 60, 300, 0]
    readonly property var timeoutLabels: [
        "5 seconds", "10 seconds", "30 seconds", "1 minute", "5 minutes", "Until logout"
    ]
    readonly property var startupValues: ["balanced", "fast_reopen", "low_memory"]
    readonly property var startupLabels: [
        "Balanced", "Prioritize fastest reopening", "Minimize background memory usage"
    ]
    readonly property var startupShortLabels: ["Balanced", "Fast reopen", "Low memory"]
    readonly property var startupHints: [
        "Reuse the process without trimming caches aggressively.",
        "Keep more state in memory so the next launch is nearly instant.",
        "Release caches when the last window closes to minimize memory use."
    ]
    readonly property var cacheLimits: [128, 256, 512, 1024, 2048]

    readonly property real uiScale: theme ? theme.font_scale : 1
    readonly property bool keepRunning: lifecycle ? lifecycle.keep_running_background : true
    readonly property bool hasError: lifecycle ? lifecycle.settings_error !== "" : false
    readonly property string statusText: lifecycle
                                        ? (lifecycle.settings_error !== ""
                                           ? lifecycle.settings_error
                                           : lifecycle.cache_clear_status)
                                        : ""

    function close() {
        open = false
    }

    function timeoutIndex() {
        if (!lifecycle)
            return 2
        var index = timeoutSeconds.indexOf(lifecycle.background_idle_timeout_seconds)
        return index >= 0 ? index : 2
    }

    function startupIndex() {
        if (!lifecycle)
            return 0
        var index = startupValues.indexOf(lifecycle.startup_behavior)
        return index >= 0 ? index : 0
    }

    function cacheIndex() {
        if (!lifecycle)
            return 2
        var index = cacheLimits.indexOf(lifecycle.thumbnail_cache_limit_mb)
        return index >= 0 ? index : 2
    }

    onOpenChanged: {
        if (open)
            forceActiveFocus()
    }

    Keys.onEscapePressed: function(event) {
        close()
        event.accepted = true
    }

    Rectangle {
        anchors.fill: parent
        color: "#000000"
        opacity: 0.55
        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }
    }

    Rectangle {
        id: card

        width: Math.min(680, root.width - 40)
        height: Math.min(680, root.height - 40)
        anchors.centerIn: parent
        radius: root.theme ? Math.max(8, root.theme.radius) : 10
        color: root.theme ? root.theme.surface : "#151820"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        // Swallow clicks so only the dimmed backdrop dismisses the dialog.
        MouseArea {
            anchors.fill: parent
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // Header -------------------------------------------------------------
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: 88

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 24
                    anchors.rightMargin: 18
                    anchors.topMargin: 18
                    anchors.bottomMargin: 14
                    spacing: 14

                    Rectangle {
                        Layout.preferredWidth: 38
                        Layout.preferredHeight: 38
                        Layout.alignment: Qt.AlignVCenter
                        radius: 9
                        color: root.theme ? root.theme.selection : "#24364f"

                        Canvas {
                            anchors.fill: parent
                            anchors.margins: 8
                            property color ink: root.theme ? root.theme.accent : "#8ab4f8"
                            onInkChanged: requestPaint()
                            Component.onCompleted: requestPaint()
                            onPaint: {
                                var ctx = getContext("2d")
                                ctx.clearRect(0, 0, width, height)
                                ctx.strokeStyle = ink
                                ctx.fillStyle = ink
                                ctx.lineWidth = 1.7
                                ctx.lineCap = "round"
                                var rows = [5, 12, 19]
                                var knobs = [15, 7, 13]
                                for (var i = 0; i < rows.length; i++) {
                                    ctx.beginPath()
                                    ctx.moveTo(2, rows[i])
                                    ctx.lineTo(width - 2, rows[i])
                                    ctx.stroke()
                                    ctx.beginPath()
                                    ctx.arc(knobs[i], rows[i], 2.4, 0, Math.PI * 2)
                                    ctx.fill()
                                }
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 3

                        Text {
                            Layout.fillWidth: true
                            text: "Performance"
                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                            font.family: root.theme ? root.theme.font_family : "sans-serif"
                            font.pixelSize: 18 * root.uiScale
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }

                        Text {
                            Layout.fillWidth: true
                            text: "Background behavior, startup, and cache management"
                            color: root.theme ? root.theme.muted : "#9299a8"
                            font.family: root.theme ? root.theme.font_family : "sans-serif"
                            font.pixelSize: 11.5 * root.uiScale
                            elide: Text.ElideRight
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

                        Accessible.name: "Close performance settings"
                        Accessible.role: Accessible.Button
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: root.theme ? root.theme.border : "#292e3a"
            }

            // Settings -----------------------------------------------------------
            ScrollView {
                id: bodyScroll
                objectName: "performanceSettingsScrollView"
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.leftMargin: 24
                Layout.rightMargin: 24
                Layout.topMargin: 16
                Layout.bottomMargin: 16
                clip: true

                ScrollBar.horizontal: ScrollBar {
                    objectName: "performanceSettingsHorizontalScrollBar"
                    policy: ScrollBar.AlwaysOff
                }

                ScrollBar.vertical: ScrollBar {
                    id: bodyBar
                    parent: bodyScroll
                    x: bodyScroll.mirrored ? 0 : bodyScroll.width - width
                    y: bodyScroll.topPadding
                    height: bodyScroll.availableHeight
                    width: 10
                    policy: ScrollBar.AsNeeded
                    visible: policy !== ScrollBar.AlwaysOff && size < 1.0

                    contentItem: Rectangle {
                        implicitWidth: 6
                        implicitHeight: 40
                        radius: width / 2
                        color: root.theme ? root.theme.muted : "#9299a8"
                        opacity: bodyBar.pressed ? 0.9 : (bodyBar.hovered ? 0.7 : 0.4)
                    }

                    background: Rectangle {
                        implicitWidth: 10
                        color: "transparent"
                    }
                }

                ColumnLayout {
                    id: bodyColumn
                    objectName: "performanceSettingsContent"
                    width: bodyScroll.availableWidth
                    spacing: 14

                    Text {
                        text: "Application lifecycle"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 10.5 * root.uiScale
                        font.weight: Font.DemiBold
                        font.letterSpacing: 0.3
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: lifecycleCard.implicitHeight
                        radius: 10
                        color: root.theme ? root.theme.surface_alt : "#1c202a"
                        border.width: 1
                        border.color: root.theme ? root.theme.border : "#292e3a"

                        ColumnLayout {
                            id: lifecycleCard
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            spacing: 0

                            RowLayout {
                                Layout.fillWidth: true
                                Layout.margins: 14
                                spacing: 18

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        Layout.fillWidth: true
                                        text: "Keep running in background"
                                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 12.5 * root.uiScale
                                        elide: Text.ElideRight
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: "Closed windows are hidden, not exited. Launching Filem again reuses this process."
                                        color: root.theme ? root.theme.muted : "#9299a8"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 11 * root.uiScale
                                        wrapMode: Text.Wrap
                                    }
                                }

                                Switch {
                                    id: keepSwitch
                                    objectName: "keepRunningBackground"
                                    Accessible.name: "Keep running in background"
                                    Layout.alignment: Qt.AlignVCenter
                                    padding: 0
                                    implicitWidth: 44
                                    implicitHeight: 26
                                    checked: root.keepRunning
                                    onToggled: if (root.lifecycle)
                                                   root.lifecycle.set_keep_running_background(checked)

                                    background: Rectangle {
                                        x: 1
                                        y: 1
                                        implicitWidth: 42
                                        implicitHeight: 24
                                        radius: 12
                                        color: keepSwitch.checked
                                               ? (root.theme ? root.theme.accent : "#8ab4f8")
                                               : (root.theme ? root.theme.surface : "#151820")
                                        border.width: 1
                                        border.color: keepSwitch.checked
                                                      ? (root.theme ? root.theme.accent : "#8ab4f8")
                                                      : (root.theme ? root.theme.border : "#292e3a")

                                        Behavior on color {
                                            ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                                        }
                                    }

                                    indicator: Rectangle {
                                        x: keepSwitch.checked ? keepSwitch.width - width - 4 : 4
                                        y: (keepSwitch.height - height) / 2
                                        width: 18
                                        height: 18
                                        radius: 9
                                        color: "#f4f6fa"

                                        Behavior on x {
                                            NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                                        }
                                    }
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                Layout.leftMargin: 14
                                Layout.rightMargin: 14
                                color: root.theme ? root.theme.border : "#292e3a"
                                opacity: 0.6
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                Layout.margins: 14
                                spacing: 18
                                opacity: root.keepRunning ? 1 : 0.5

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        Layout.fillWidth: true
                                        text: "Background idle timeout"
                                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 12.5 * root.uiScale
                                        elide: Text.ElideRight
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: "Release the hidden process after this much idle time. Until logout keeps it available all session."
                                        color: root.theme ? root.theme.muted : "#9299a8"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 11 * root.uiScale
                                        wrapMode: Text.Wrap
                                    }
                                }

                                ComboBox {
                                    id: timeoutControl
                                    objectName: "backgroundIdleTimeout"
                                    Accessible.name: "Background idle timeout"
                                    Layout.preferredWidth: 190
                                    Layout.alignment: Qt.AlignVCenter
                                    enabled: root.keepRunning && !!root.lifecycle
                                    model: root.timeoutLabels
                                    currentIndex: root.timeoutIndex()
                                    onActivated: function(index) {
                                        if (root.lifecycle)
                                            root.lifecycle.set_background_idle_timeout(root.timeoutSeconds[index])
                                    }

                                    contentItem: Text {
                                        leftPadding: 10
                                        rightPadding: timeoutControl.indicator.width + 10
                                        text: timeoutControl.displayText
                                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 12 * root.uiScale
                                        verticalAlignment: Text.AlignVCenter
                                        elide: Text.ElideRight
                                    }

                                    indicator: Canvas {
                                        x: timeoutControl.width - width - 10
                                        y: (timeoutControl.height - height) / 2
                                        width: 10
                                        height: 6
                                        property color ink: root.theme ? root.theme.muted : "#9299a8"
                                        onInkChanged: requestPaint()
                                        Component.onCompleted: requestPaint()
                                        onPaint: {
                                            var ctx = getContext("2d")
                                            ctx.clearRect(0, 0, width, height)
                                            ctx.strokeStyle = ink
                                            ctx.lineWidth = 1.5
                                            ctx.lineCap = "round"
                                            ctx.lineJoin = "round"
                                            ctx.beginPath()
                                            ctx.moveTo(1, 1)
                                            ctx.lineTo(width / 2, height - 1)
                                            ctx.lineTo(width - 1, 1)
                                            ctx.stroke()
                                        }
                                    }

                                    background: Rectangle {
                                        radius: 7
                                        color: root.theme ? root.theme.surface : "#151820"
                                        border.width: 1
                                        border.color: timeoutControl.activeFocus
                                                      ? (root.theme ? root.theme.focus : "#8ab4f8")
                                                      : (root.theme ? root.theme.border : "#292e3a")
                                    }

                                    delegate: ItemDelegate {
                                        id: timeoutDelegate
                                        required property var modelData
                                        width: timeoutControl.width
                                        contentItem: Text {
                                            text: timeoutDelegate.modelData
                                            rightPadding: 14
                                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                                            font.family: root.theme ? root.theme.font_family : "sans-serif"
                                            font.pixelSize: Math.round(12 * root.uiScale)
                                            verticalAlignment: Text.AlignVCenter
                                        }
                                        background: Rectangle {
                                            color: timeoutDelegate.highlighted
                                                   ? (root.theme ? root.theme.hover : "#202531")
                                                   : (root.theme ? root.theme.surface_alt : "#1c202a")
                                        }
                                    }

                                    popup: Popup {
                                        y: timeoutControl.height - 1
                                        width: timeoutControl.width
                                        implicitHeight: contentItem.implicitHeight
                                        padding: 1
                                        contentItem: ListView {
                                            clip: true
                                            implicitHeight: Math.min(contentHeight, 260)
                                            model: timeoutControl.popup.visible ? timeoutControl.delegateModel : null
                                            currentIndex: timeoutControl.highlightedIndex
                                            boundsBehavior: Flickable.StopAtBounds
                                            ScrollBar.vertical: ScrollBar {
                                                policy: ScrollBar.AsNeeded
                                                width: 8
                                            }
                                        }
                                        background: Rectangle {
                                            color: root.theme ? root.theme.surface_alt : "#1c202a"
                                            border.color: root.theme ? root.theme.border : "#292e3a"
                                            radius: 7
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        Layout.topMargin: 2
                        text: "Startup behavior"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 10.5 * root.uiScale
                        font.weight: Font.DemiBold
                        font.letterSpacing: 0.3
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: startupCard.implicitHeight
                        radius: 10
                        color: root.theme ? root.theme.surface_alt : "#1c202a"
                        border.width: 1
                        border.color: root.theme ? root.theme.border : "#292e3a"

                        ColumnLayout {
                            id: startupCard
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            spacing: 10

                            Item {
                                objectName: "startupBehavior"
                                Layout.fillWidth: true
                                Layout.preferredHeight: 106
                                Layout.leftMargin: 14
                                Layout.rightMargin: 14
                                Layout.topMargin: 14
                                Layout.bottomMargin: 14

                                Row {
                                    id: startupRow
                                    anchors.fill: parent
                                    spacing: 8

                                    Repeater {
                                        model: root.startupShortLabels

                                        delegate: Rectangle {
                                            id: startupSegment
                                            required property string modelData
                                            required property int index

                                            width: (startupRow.width - (root.startupShortLabels.length - 1) * startupRow.spacing)
                                                   / root.startupShortLabels.length
                                            height: startupRow.height
                                            radius: 9
                                            color: root.startupIndex() === startupSegment.index
                                                   ? (root.theme ? root.theme.selection : "#24364f")
                                                   : (startupMouse.containsMouse
                                                      ? (root.theme ? root.theme.hover : "#202531")
                                                      : (root.theme ? root.theme.surface : "#151820"))
                                            border.width: 1
                                            border.color: root.startupIndex() === startupSegment.index
                                                          ? (root.theme ? root.theme.accent : "#8ab4f8")
                                                          : (root.theme ? root.theme.border : "#292e3a")

                                            Behavior on color {
                                                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                                            }

                                            Column {
                                                anchors.fill: parent
                                                anchors.margins: 10
                                                spacing: 5

                                                Text {
                                                    width: parent.width
                                                    text: startupSegment.modelData
                                                    color: root.startupIndex() === startupSegment.index
                                                           ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                                                           : (root.theme ? root.theme.foreground : "#e7e9ee")
                                                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                                                    font.pixelSize: 11.5 * root.uiScale
                                                    font.weight: Font.DemiBold
                                                }

                                                Text {
                                                    width: parent.width
                                                    height: parent.height - 25
                                                    text: root.startupHints[startupSegment.index]
                                                    color: root.startupIndex() === startupSegment.index
                                                           ? (root.theme ? root.theme.selection_text : "#f2f5fb")
                                                           : (root.theme ? root.theme.muted : "#9299a8")
                                                    opacity: root.startupIndex() === startupSegment.index ? 0.9 : 1
                                                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                                                    font.pixelSize: 10 * root.uiScale
                                                    wrapMode: Text.Wrap
                                                }
                                            }

                                            MouseArea {
                                                id: startupMouse
                                                anchors.fill: parent
                                                hoverEnabled: true
                                                cursorShape: Qt.PointingHandCursor
                                                onClicked: if (root.lifecycle)
                                                               root.lifecycle.set_startup_behavior(root.startupValues[startupSegment.index])
                                            }

                                            Accessible.name: startupSegment.modelData + ". "
                                                            + root.startupHints[startupSegment.index]
                                            Accessible.role: Accessible.RadioButton
                                            Accessible.checked: root.startupIndex() === startupSegment.index
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        Layout.topMargin: 2
                        text: "Cache management"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 10.5 * root.uiScale
                        font.weight: Font.DemiBold
                        font.letterSpacing: 0.3
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: cacheCard.implicitHeight
                        radius: 10
                        color: root.theme ? root.theme.surface_alt : "#1c202a"
                        border.width: 1
                        border.color: root.theme ? root.theme.border : "#292e3a"

                        ColumnLayout {
                            id: cacheCard
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            spacing: 0

                            ColumnLayout {
                                Layout.fillWidth: true
                                Layout.margins: 14
                                spacing: 3

                                Text {
                                    Layout.fillWidth: true
                                    text: "Thumbnail cache limit"
                                    color: root.theme ? root.theme.foreground : "#e7e9ee"
                                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                                    font.pixelSize: 12.5 * root.uiScale
                                    elide: Text.ElideRight
                                }

                                Text {
                                    Layout.fillWidth: true
                                    text: "Older thumbnails are evicted as new ones are generated. Valid entries are reused until source files change."
                                    color: root.theme ? root.theme.muted : "#9299a8"
                                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                                    font.pixelSize: 11 * root.uiScale
                                    wrapMode: Text.Wrap
                                }
                            }

                            RowLayout {
                                objectName: "thumbnailCacheLimit"
                                Layout.fillWidth: true
                                Layout.leftMargin: 14
                                Layout.rightMargin: 14
                                Layout.bottomMargin: 14
                                spacing: 6

                                Repeater {
                                    model: root.cacheLimits

                                    delegate: ActionButton {
                                        required property int modelData
                                        required property int index
                                        objectName: "thumbnailCacheOption_" + modelData
                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 34
                                        theme: root.theme
                                        enabled: !!root.lifecycle
                                        emphasized: root.cacheIndex() === index
                                        text: modelData >= 1024
                                              ? (modelData / 1024) + " GB"
                                              : modelData + " MB"
                                        Accessible.name: modelData + " megabytes thumbnail cache limit"
                                        onClicked: if (root.lifecycle)
                                                       root.lifecycle.set_thumbnail_cache_limit(modelData)
                                    }
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                Layout.leftMargin: 14
                                Layout.rightMargin: 14
                                color: root.theme ? root.theme.border : "#292e3a"
                                opacity: 0.6
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                Layout.margins: 14
                                spacing: 18

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        Layout.fillWidth: true
                                        text: "Clear thumbnail cache"
                                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 12.5 * root.uiScale
                                        elide: Text.ElideRight
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: "Removes only thumbnails generated by Filem. Other applications keep their caches."
                                        color: root.theme ? root.theme.muted : "#9299a8"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 11 * root.uiScale
                                        wrapMode: Text.Wrap
                                    }
                                }

                                Button {
                                    id: clearButton
                                    objectName: "clearThumbnailCache"
                                    Accessible.name: "Clear thumbnail cache"
                                    Layout.alignment: Qt.AlignVCenter
                                    enabled: !!root.lifecycle
                                    padding: 0
                                    implicitWidth: clearLabel.implicitWidth + 28
                                    implicitHeight: 30
                                    onClicked: if (root.lifecycle)
                                                   root.lifecycle.clear_thumbnail_cache()

                                    contentItem: Text {
                                        id: clearLabel
                                        text: "Clear"
                                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                                        font.pixelSize: 12 * root.uiScale
                                        font.weight: Font.DemiBold
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                    }

                                    background: Rectangle {
                                        radius: 7
                                        color: clearButton.hovered
                                               ? (root.theme ? root.theme.hover : "#202531")
                                               : (root.theme ? root.theme.surface : "#151820")
                                        border.width: 1
                                        border.color: root.theme ? root.theme.border : "#292e3a"
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        Layout.topMargin: 2
                        text: "File metadata uses a bounded 96-entry memory cache. Directory listings are read asynchronously and refreshed by filesystem notifications. Icon resources are reused with a bounded process-wide cache."
                        wrapMode: Text.Wrap
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 10.5 * root.uiScale
                    }
                }
            }

            // Footer -------------------------------------------------------------
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: 60

                Rectangle {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    height: 1
                    color: root.theme ? root.theme.border : "#292e3a"
                }

                Text {
                    objectName: "performanceStatus"
                    anchors.left: parent.left
                    anchors.leftMargin: 24
                    anchors.right: doneButton.left
                    anchors.rightMargin: 16
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.statusText
                    visible: text !== ""
                    elide: Text.ElideRight
                    color: root.hasError
                           ? (root.theme ? root.theme.danger : "#f28b82")
                           : (root.theme ? root.theme.muted : "#9299a8")
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 10.5 * root.uiScale
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

                    Accessible.name: "Close performance settings"
                    Accessible.role: Accessible.Button
                }
            }
        }
    }

    Timer {
        interval: 250
        repeat: true
        running: root.open && !!root.lifecycle
        onTriggered: root.lifecycle.poll_cache_clear_status()
    }
}
