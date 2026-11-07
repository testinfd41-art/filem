import QtQuick

Rectangle {
    id: root

    width: minimized
           ? Math.min(380, Math.ceil(panelTitleMetrics.advanceWidth + panelControls.implicitWidth + 29))
           : 360
    height: minimized ? 48 : 112
    radius: theme ? Math.max(6, theme.radius) : 8
    color: theme ? theme.surface : "#151820"
    border.width: 1
    border.color: theme ? theme.border : "#292e3a"

    property var theme
    property var backend
    property var manager
    property string dismissedJobId: ""
    property bool minimized: false
    readonly property var jobs: backend ? backend.transfer_jobs : []
    readonly property var activeJob: {
        for (var i = 0; i < jobs.length; i++) {
            if (jobs[i].status === "running")
                return jobs[i]
        }
        for (var j = 0; j < jobs.length; j++) {
            if (jobs[j].status === "queued")
                return jobs[j]
        }
        return jobs.length > 0 ? jobs[jobs.length - 1] : null
    }
    readonly property bool hasQueueJobs: jobs.length > 0
    readonly property bool terminalJob: activeJob
                                         && (activeJob.status === "completed"
                                             || activeJob.status === "failed"
                                             || activeJob.status === "cancelled")
    readonly property string transferMetrics: activeJob
                                               ? activeJob.completedFiles + " / "
                                                 + activeJob.totalFiles + " files · "
                                                 + Math.round(activeJob.progress * 100) + "%"
                                                 + (activeJob.bytesPerSecond > 0
                                                    ? " · " + formatSpeed(activeJob.bytesPerSecond)
                                                    : "")
                                                 + (activeJob.status === "running" && activeJob.etaSeconds >= 0
                                                    ? " · " + formatEta(activeJob.etaSeconds)
                                                    : "")
                                               : ""
    readonly property bool showingLegacyOperation: backend !== null
                                                     && backend !== undefined
                                                     && backend.op_active === true
                                                     && (!activeJob
                                                         || (activeJob.status !== "running"
                                                             && activeJob.status !== "queued"))
    readonly property bool shouldShow: (hasQueueJobs
                                        && !(terminalJob
                                             && dismissedJobId === String(activeJob.id)))
                                       || (backend !== null && backend !== undefined
                                           && backend.op_active === true)

    visible: shouldShow

    TextMetrics {
        id: panelTitleMetrics
        font: panelTitle.font
        text: panelTitle.text
    }

    function formatSpeed(bytesPerSecond) {
        if (bytesPerSecond >= 1024 * 1024)
            return (bytesPerSecond / (1024 * 1024)).toFixed(1) + " MB/s"
        if (bytesPerSecond >= 1024)
            return Math.round(bytesPerSecond / 1024) + " KB/s"
        return bytesPerSecond + " B/s"
    }

    function formatEta(seconds) {
        if (seconds === undefined || seconds === null || seconds < 0)
            return ""
        if (seconds === 0)
            return "0s left"
        if (seconds < 60)
            return seconds + "s left"
        var mins = Math.floor(seconds / 60)
        var secs = seconds % 60
        if (mins < 60)
            return secs > 0 ? (mins + "m " + secs + "s left") : (mins + "m left")
        var hours = Math.floor(mins / 60)
        var remMins = mins % 60
        return remMins > 0 ? (hours + "h " + remMins + "m left") : (hours + "h left")
    }

    function dismissCompletedJob() {
        if (terminalJob && activeJob)
            dismissedJobId = String(activeJob.id)
    }

    function minimize() {
        minimized = true
    }

    function restore() {
        minimized = false
    }

    function openManager() {
        if (!hasQueueJobs || !manager)
            return
        manager.open = true
        if (backend)
            backend.refresh_transfer_jobs()
    }

    MouseArea {
        anchors.fill: parent
        enabled: root.hasQueueJobs || root.minimized
        cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: {
            if (root.minimized)
                root.restore()
            else
                root.openManager()
        }
    }

    Column {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 7

        Row {
            width: parent.width
            height: 22
            spacing: 5

            Text {
                id: panelTitle
                width: parent.width - panelControls.width
                height: parent.height
                verticalAlignment: Text.AlignVCenter
                text: {
                    if (root.showingLegacyOperation)
                        return root.backend.op_title
                               + (root.minimized
                                  ? " · " + Math.round(root.backend.op_progress * 100) + "%"
                                  : "")
                    if (!root.activeJob)
                        return "Transfer Manager"
                    if (root.minimized)
                        return root.activeJob.operation + " · "
                               + Math.round(root.activeJob.progress * 100) + "%"
                               + (root.activeJob.status === "running" && root.activeJob.etaSeconds >= 0
                                  ? " · " + root.formatEta(root.activeJob.etaSeconds)
                                  : "")
                    return root.activeJob.operation + " · "
                           + (root.activeJob.status === "running"
                              ? "in progress"
                              : root.activeJob.status === "queued"
                                ? "queued"
                                : root.activeJob.status)
                }
                elide: Text.ElideRight
                color: theme ? theme.foreground : "#e7e9ee"
                font.family: theme ? theme.font_family : "sans-serif"
                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                font.weight: Font.DemiBold
            }

            Row {
                id: panelControls
                height: parent.height
                spacing: 4
                layoutDirection: Qt.RightToLeft

                Item {
                    id: actionItem
                    objectName: "progressPanelAction"
                    Accessible.name: root.terminalJob
                                    ? "Dismiss completed transfer"
                                    : "Cancel transfer"
                    width: root.terminalJob && !root.showingLegacyOperation ? 28 : 58
                    height: parent.height
                    visible: root.showingLegacyOperation
                             ? root.backend.op_cancelable
                             : (root.activeJob && (root.activeJob.status === "running"
                                                   || root.activeJob.status === "queued"
                                                   || root.terminalJob))
                    z: 2

                    Rectangle {
                        objectName: "progressPanelActionBackground"
                        anchors.fill: parent
                        radius: theme ? Math.max(3, theme.radius * 0.45) : 5
                        color: root.terminalJob && !root.showingLegacyOperation
                               ? (theme ? theme.surface_alt : "#1c202a")
                               : (actionMouse.containsMouse
                                  ? (theme ? theme.hover : "#202531")
                                  : "transparent")
                        border.width: 1
                        border.color: root.terminalJob && !root.showingLegacyOperation
                                      ? (theme ? theme.muted : "#9299a8")
                                      : (theme ? theme.border : "#292e3a")

                        Text {
                            objectName: "progressPanelActionText"
                            anchors.centerIn: parent
                            text: root.terminalJob && !root.showingLegacyOperation ? "×" : "Cancel"
                            color: theme ? theme.foreground : "#e7e9ee"
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: (root.terminalJob
                                             && !root.showingLegacyOperation ? 16 : 11)
                                            * (theme ? theme.font_scale : 1)
                        }

                        MouseArea {
                            id: actionMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                if (!root.backend)
                                    return
                                if (root.terminalJob && !root.showingLegacyOperation) {
                                    root.dismissCompletedJob()
                                } else if (root.showingLegacyOperation) {
                                    root.backend.cancel_operation()
                                } else if (root.activeJob) {
                                    root.backend.cancel_transfer_job(root.activeJob.id)
                                }
                            }
                        }
                    }
                }

                Item {
                    id: minimizeItem
                    objectName: "progressPanelMinimize"
                    Accessible.name: "Minimize transfer progress"
                    width: 28
                    height: parent.height
                    visible: !root.minimized
                             && (root.hasQueueJobs || root.showingLegacyOperation)
                    z: 2

                    Rectangle {
                        anchors.fill: parent
                        radius: theme ? Math.max(3, theme.radius * 0.45) : 5
                        color: minimizeMouse.containsMouse
                               ? (theme ? theme.hover : "#202531")
                               : "transparent"
                        border.width: 1
                        border.color: theme ? theme.border : "#292e3a"

                        Text {
                            objectName: "progressPanelMinimizeText"
                            anchors.centerIn: parent
                            text: "⌄"
                            color: theme ? theme.foreground : "#e7e9ee"
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                        }

                        MouseArea {
                            id: minimizeMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.minimize()
                        }
                    }
                }
            }
        }

        Text {
            width: parent.width
            visible: !root.minimized
            text: root.showingLegacyOperation
                  ? root.backend.op_detail
                  : (root.activeJob && root.activeJob.status === "running"
                     ? root.activeJob.currentItem
                     : (root.activeJob && root.activeJob.status === "queued"
                        ? root.activeJob.sources : (root.activeJob ? root.activeJob.summary : "")))
            elide: Text.ElideMiddle
            color: theme ? theme.muted : "#9299a8"
            font.family: theme ? theme.mono_font : "monospace"
            font.pixelSize: 10 * (theme ? theme.font_scale : 1)
        }

        Rectangle {
            width: parent.width
            height: 4
            visible: !root.minimized
            radius: 2
            color: theme ? theme.border : "#292e3a"

            Rectangle {
                width: parent.width * Math.max(
                           0,
                           Math.min(1, root.showingLegacyOperation
                                    ? root.backend.op_progress
                                    : (root.activeJob ? root.activeJob.progress : 0)))
                height: parent.height
                radius: 2
                color: theme ? theme.accent : "#8ab4f8"
                Behavior on width { NumberAnimation { duration: 140 } }
            }
        }

        Row {
            width: parent.width
            height: 22
            visible: !root.minimized

            Text {
                objectName: "compactTransferMetrics"
                width: parent.width
                anchors.verticalCenter: parent.verticalCenter
                text: root.showingLegacyOperation
                      ? Math.round(root.backend.op_progress * 100) + "%"
                      : (root.activeJob && (root.activeJob.status === "running"
                                            || root.terminalJob)
                         ? root.transferMetrics
                         : (root.activeJob ? root.activeJob.summary : ""))
                color: theme ? theme.muted : "#9299a8"
                font.family: theme ? theme.mono_font : "monospace"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                elide: Text.ElideRight
            }
        }
    }
}
