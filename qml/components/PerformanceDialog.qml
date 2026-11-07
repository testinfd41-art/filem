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
    readonly property var cacheLimits: [128, 256, 512, 1024, 2048]

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
        width: Math.min(640, root.width - 40)
        height: Math.min(620, root.height - 40)
        anchors.centerIn: parent
        radius: root.theme ? Math.max(6, root.theme.radius) : 10
        color: root.theme ? root.theme.surface : "#151820"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 14

            RowLayout {
                Layout.fillWidth: true
                Text {
                    Layout.fillWidth: true
                    text: "Performance"
                    color: root.theme ? root.theme.foreground : "#e7e9ee"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 17 * (root.theme ? root.theme.font_scale : 1)
                    font.weight: Font.DemiBold
                }
                ToolButton {
                    text: "×"
                    Accessible.name: "Close performance settings"
                    onClicked: root.close()
                }
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                ColumnLayout {
                    width: parent.width
                    spacing: 12

                    Text {
                        text: "Application lifecycle"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: "Keep running in background"
                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                            font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                        }
                        Switch {
                            objectName: "keepRunningBackground"
                            Accessible.name: "Keep running in background"
                            checked: root.lifecycle ? root.lifecycle.keep_running_background : true
                            onToggled: if (root.lifecycle)
                                           root.lifecycle.set_keep_running_background(checked)
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: "Background idle timeout"
                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                            font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                        }
                        ComboBox {
                            objectName: "backgroundIdleTimeout"
                            Accessible.name: "Background idle timeout"
                            Layout.preferredWidth: 210
                            model: root.timeoutLabels
                            currentIndex: root.timeoutIndex()
                            onActivated: function(index) {
                                if (root.lifecycle)
                                    root.lifecycle.set_background_idle_timeout(root.timeoutSeconds[index])
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: "A closed window is hidden and released from active directory monitoring. Launching Filem again during this period reuses this process."
                        wrapMode: Text.Wrap
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.pixelSize: 11 * (root.theme ? root.theme.font_scale : 1)
                    }

                    Text {
                        Layout.topMargin: 8
                        text: "Startup behavior"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }

                    ComboBox {
                        objectName: "startupBehavior"
                        Accessible.name: "Startup behavior"
                        Layout.fillWidth: true
                        model: root.startupLabels
                        currentIndex: root.startupIndex()
                        onActivated: function(index) {
                            if (root.lifecycle)
                                root.lifecycle.set_startup_behavior(root.startupValues[index])
                        }
                    }

                    Text {
                        Layout.topMargin: 8
                        text: "Cache management"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: "Thumbnail cache limit"
                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                            font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                        }
                        ComboBox {
                            objectName: "thumbnailCacheLimit"
                            Accessible.name: "Thumbnail cache limit"
                            Layout.preferredWidth: 210
                            model: root.cacheLimits.map(function(value) { return value + " MB" })
                            currentIndex: root.cacheIndex()
                            onActivated: function(index) {
                                if (root.lifecycle)
                                    root.lifecycle.set_thumbnail_cache_limit(root.cacheLimits[index])
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: "Valid thumbnails are reused and invalidated when source files change. Cleanup only removes Filem-owned thumbnail entries."
                            wrapMode: Text.Wrap
                            color: root.theme ? root.theme.muted : "#9299a8"
                            font.pixelSize: 11 * (root.theme ? root.theme.font_scale : 1)
                        }
                        Button {
                            text: "Clear"
                            enabled: !!root.lifecycle
                            onClicked: root.lifecycle.clear_thumbnail_cache()
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: "File metadata uses a bounded 96-entry memory cache. Directory listings are read asynchronously and refreshed by filesystem notifications. Icon resources are reused with a bounded process-wide cache."
                        wrapMode: Text.Wrap
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.pixelSize: 11 * (root.theme ? root.theme.font_scale : 1)
                    }

                    Text {
                        objectName: "performanceStatus"
                        Layout.fillWidth: true
                        text: root.lifecycle
                              ? (root.lifecycle.settings_error !== ""
                                 ? root.lifecycle.settings_error
                                 : root.lifecycle.cache_clear_status)
                              : ""
                        visible: text !== ""
                        wrapMode: Text.Wrap
                        color: root.theme ? root.theme.danger : "#f28b82"
                        font.pixelSize: 11 * (root.theme ? root.theme.font_scale : 1)
                    }
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
