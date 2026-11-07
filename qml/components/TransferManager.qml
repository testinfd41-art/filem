import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    anchors.fill: parent
    visible: open
    z: 450
    focus: open

    property var theme
    property var backend
    property bool open: false
    property string expandedJobId: ""
    property var expandedItems: []
    property real jobsWheelTargetY: 0
    property bool detailsListHovered: false

    function close() {
        open = false
        expandedJobId = ""
        expandedItems = []
    }

    function jobsFor(status) {
        var jobs = backend ? backend.transfer_jobs : []
        var result = []
        for (var i = 0; i < jobs.length; i++) {
            if (jobs[i].status === status)
                result.push(jobs[i])
        }
        return result
    }

    function toggleDetails(job) {
        var jobId = String(job.id)
        if (expandedJobId === jobId) {
            expandedJobId = ""
            expandedItems = []
        } else {
            expandedJobId = jobId
            expandedItems = backend ? backend.transfer_job_items(jobId) : []
        }
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

    function scrollJobsByWheel(delta) {
        if (!jobsWheelAnimation.running)
            jobsWheelTargetY = jobsFlickable.contentY
        var maxY = Math.max(0, jobsFlickable.contentHeight - jobsFlickable.height)
        jobsWheelTargetY = Math.max(0, Math.min(maxY, jobsWheelTargetY - delta))
        jobsWheelAnimation.from = jobsFlickable.contentY
        jobsWheelAnimation.to = jobsWheelTargetY
        jobsWheelAnimation.restart()
    }

    onOpenChanged: {
        if (open) {
            if (backend)
                backend.refresh_transfer_jobs()
            forceActiveFocus()
        }
    }

    Keys.onEscapePressed: function(event) {
        close()
        event.accepted = true
    }
    Keys.priority: Keys.BeforeItem

    Timer {
        interval: 600
        repeat: true
        running: root.open && root.expandedJobId !== ""
        onTriggered: {
            if (root.backend) {
                var items = root.backend.transfer_job_items(root.expandedJobId)
                if (items.length > 500)
                    items = items.slice(0, 500)
                root.expandedItems = items
            }
        }
    }

    Rectangle {
        anchors.fill: parent
        color: "#000000"
        opacity: 0.4
        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }
    }

    Rectangle {
        objectName: "transferManagerPanel"
        id: panel
        width: Math.min(780, root.width - 36)
        height: Math.min(650, root.height - 36)
        anchors.centerIn: parent
        radius: root.theme ? Math.max(6, root.theme.radius) : 9
        color: root.theme ? root.theme.surface_alt : "#1c202a"
        border.width: 1
        border.color: root.theme ? root.theme.border : "#292e3a"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 18
            spacing: 12

            RowLayout {
                Layout.fillWidth: true
                Text {
                    Layout.fillWidth: true
                    text: "Transfer Manager"
                    color: root.theme ? root.theme.foreground : "#e7e9ee"
                    font.family: root.theme ? root.theme.font_family : "sans-serif"
                    font.pixelSize: 17 * (root.theme ? root.theme.font_scale : 1)
                    font.weight: Font.DemiBold
                }
                ActionButton {
                    objectName: "cancelQueuedTransfersButton"
                    theme: root.theme
                    text: "Cancel queued"
                    enabled: root.backend && root.jobsFor("queued").length > 0
                    Accessible.name: "Cancel all queued transfers"
                    onClicked: root.backend.cancel_pending_transfers()
                }
                ActionButton {
                    objectName: "clearCompletedTransfersButton"
                    theme: root.theme
                    text: "Clear completed"
                    enabled: root.backend && root.jobsFor("completed").length > 0
                    Accessible.name: "Clear completed transfers"
                    onClicked: root.backend.clear_completed_transfers()
                }
                Rectangle {
                    objectName: "closeTransferManagerButton"
                    width: 34
                    height: 34
                    radius: root.theme ? Math.max(3, root.theme.radius * 0.45) : 5
                    color: closeMouse.containsMouse
                           ? (root.theme ? root.theme.hover : "#303746")
                           : (root.theme ? root.theme.surface : "#151820")
                    border.width: 1
                    border.color: root.theme ? root.theme.border : "#454b58"
                    Accessible.name: "Close Transfer Manager"

                    Text {
                        objectName: "closeTransferManagerGlyph"
                        anchors.centerIn: parent
                        text: "×"
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 16 * (root.theme ? root.theme.font_scale : 1)
                    }

                    MouseArea {
                        id: closeMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.close()
                    }
                }
            }

            Flickable {
                id: jobsFlickable
                objectName: "transferJobsFlickable"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: width
                contentHeight: jobsColumn.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                maximumFlickVelocity: 12000
                flickDeceleration: 9000

                ScrollBar.vertical: ScrollBar {
                    policy: ScrollBar.AsNeeded
                }

                WheelHandler {
                    enabled: !root.detailsListHovered
                    acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                    onWheel: function(event) {
                        var delta = event.pixelDelta.y !== 0
                                    ? event.pixelDelta.y
                                    : (event.angleDelta.y / 120) * 120
                        root.scrollJobsByWheel(delta)
                        event.accepted = true
                    }
                }

                ColumnLayout {
                    id: jobsColumn
                    width: jobsFlickable.width
                    spacing: 14

                    Text {
                        visible: root.jobsFor("running").length > 0
                        text: "Active"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }
                    Repeater {
                        model: root.jobsFor("running")
                        delegate: jobCard
                    }

                    Text {
                        visible: root.jobsFor("queued").length > 0
                        text: "Queued"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }
                    Repeater {
                        model: root.jobsFor("queued")
                        delegate: jobCard
                    }

                    Text {
                        visible: root.jobsFor("completed").length > 0
                        text: "Completed"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }
                    Repeater {
                        model: root.jobsFor("completed")
                        delegate: jobCard
                    }

                    Text {
                        visible: root.jobsFor("failed").length > 0
                        text: "Failed"
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }
                    Repeater {
                        model: root.jobsFor("failed")
                        delegate: jobCard
                    }

                    Text {
                        visible: root.jobsFor("cancelled").length > 0
                        text: "Cancelled"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.pixelSize: 13 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }
                    Repeater {
                        model: root.jobsFor("cancelled")
                        delegate: jobCard
                    }

                    Text {
                        visible: root.backend && root.backend.transfer_jobs.length === 0
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        text: "No transfer jobs yet"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                    }
                }
            }
        }
    }

    NumberAnimation {
        id: jobsWheelAnimation
        target: jobsFlickable
        property: "contentY"
        duration: 180
        easing.type: Easing.OutCubic
    }

    Component {
        id: jobCard

        Rectangle {
            id: card
            property var job: modelData
            objectName: "transferJobCard_" + String(job.id)
            readonly property bool expanded: root.expandedJobId === String(job.id)
            readonly property bool isActive: job.status === "running"
            width: parent ? parent.width : 0
            implicitHeight: column.implicitHeight + 20
            radius: root.theme ? Math.max(4, root.theme.radius * 0.65) : 6
            color: root.theme ? root.theme.background : "#101217"
            border.width: 1
            border.color: root.theme ? root.theme.border : "#292e3a"

            ColumnLayout {
                id: column
                anchors.fill: parent
                anchors.margins: 10
                spacing: 6

                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        Layout.fillWidth: true
                        text: card.job.operation + " · " + card.job.status
                        color: root.theme ? root.theme.foreground : "#e7e9ee"
                        font.family: root.theme ? root.theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (root.theme ? root.theme.font_scale : 1)
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }
                    ActionButton {
                        theme: root.theme
                        text: card.expanded ? "Hide details" : "Details"
                        objectName: "transferDetailsButton_" + String(card.job.id)
                        Accessible.name: (card.expanded ? "Hide" : "Show")
                                        + " details for " + card.job.operation + " transfer"
                        onClicked: root.toggleDetails(card.job)
                    }
                    ActionButton {
                        theme: root.theme
                        visible: card.job.status === "queued" || card.isActive
                        text: "Cancel"
                        Accessible.name: "Cancel " + card.job.operation + " job"
                        onClicked: root.backend.cancel_transfer_job(card.job.id)
                    }
                    ActionButton {
                        theme: root.theme
                        visible: card.job.status === "failed"
                                 && card.job.operation !== "Restore"
                        text: "Retry"
                        Accessible.name: "Retry failed " + card.job.operation + " job"
                        onClicked: root.backend.retry_transfer_job(card.job.id)
                    }
                }

                Text {
                    Layout.fillWidth: true
                    visible: card.isActive || card.job.status === "queued"
                    text: card.isActive && card.job.currentItem !== ""
                          ? card.job.currentItem
                          : (card.job.sources
                             + (card.job.destination !== "" ? " → " + card.job.destination : ""))
                    color: root.theme ? root.theme.muted : "#9299a8"
                    font.family: root.theme ? root.theme.mono_font : "monospace"
                    font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                    elide: Text.ElideMiddle
                }

                RowLayout {
                    visible: card.isActive
                    Layout.fillWidth: true
                    Text {
                        Layout.fillWidth: true
                        text: card.job.completedFiles + " / " + card.job.totalFiles
                              + " files · " + card.job.completedFolders + " / "
                              + card.job.totalFolders + " folders · "
                              + Math.round(card.job.progress * 100) + "%"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                    }
                    Text {
                        visible: card.job.bytesPerSecond > 0
                        text: Math.round(card.job.bytesPerSecond / 1024) + " KB/s"
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.mono_font : "monospace"
                        font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                    }
                    Text {
                        visible: card.job.etaSeconds >= 0
                        text: root.formatEta(card.job.etaSeconds)
                        color: root.theme ? root.theme.muted : "#9299a8"
                        font.family: root.theme ? root.theme.mono_font : "monospace"
                        font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                    }
                }

                Rectangle {
                    visible: card.isActive
                    Layout.fillWidth: true
                    height: 4
                    radius: 2
                    color: root.theme ? root.theme.border : "#292e3a"
                    Rectangle {
                        width: parent.width * Math.max(0, Math.min(1, card.job.progress))
                        height: parent.height
                        radius: parent.radius
                        color: root.theme ? root.theme.accent : "#8ab4f8"
                        Behavior on width { NumberAnimation { duration: 140 } }
                    }
                }

                Text {
                    visible: card.job.summary !== "" && !card.isActive
                    Layout.fillWidth: true
                    text: card.job.summary
                    color: root.theme ? root.theme.muted : "#9299a8"
                    font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                    wrapMode: Text.Wrap
                }
                Text {
                    visible: card.job.error !== ""
                    Layout.fillWidth: true
                    text: card.job.error
                    color: root.theme ? root.theme.danger : "#ff6b6b"
                    font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                    wrapMode: Text.Wrap
                }

                ListView {
                    objectName: "transferDetailsList_" + String(card.job.id)
                    visible: card.expanded
                    Layout.fillWidth: true
                    Layout.minimumHeight: 0
                    Layout.preferredHeight: Math.min(contentHeight, 220)
                    clip: true
                    model: card.expanded ? root.expandedItems : []
                    ScrollBar.vertical: ScrollBar {
                        policy: ScrollBar.AsNeeded
                    }
                    HoverHandler {
                        onHoveredChanged: root.detailsListHovered = hovered
                    }
                    delegate: ColumnLayout {
                        width: ListView.view.width
                        spacing: 3
                        Text {
                            Layout.fillWidth: true
                            text: modelData.path || modelData.name
                            color: root.theme ? root.theme.foreground : "#e7e9ee"
                            font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                            wrapMode: Text.WrapAnywhere
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            Text {
                                text: modelData.status
                                color: modelData.status === "failed"
                                       ? (root.theme ? root.theme.danger : "#ff6b6b")
                                       : (root.theme ? root.theme.muted : "#9299a8")
                                font.pixelSize: 10 * (root.theme ? root.theme.font_scale : 1)
                            }
                            Text {
                                Layout.fillWidth: true
                                visible: modelData.error !== ""
                                text: modelData.error
                                color: root.theme ? root.theme.danger : "#ff6b6b"
                                font.pixelSize: 9 * (root.theme ? root.theme.font_scale : 1)
                                wrapMode: Text.WrapAnywhere
                            }
                        }
                    }
                }
                Text {
                    visible: card.expanded && card.job.omittedItemDetails > 0
                    text: card.job.omittedItemDetails + " additional items omitted to limit memory use"
                    color: root.theme ? root.theme.muted : "#9299a8"
                    font.pixelSize: 9 * (root.theme ? root.theme.font_scale : 1)
                }
            }
        }
    }
}
