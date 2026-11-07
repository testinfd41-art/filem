import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    name: "TransferManager"
    when: windowShown
    width: 900
    height: 640

    QtObject {
        id: fakeBackend
        property var transfer_jobs: []
        property var detailItems: []
        property string cancelledId: ""
        property string retriedId: ""
        property int cancelPendingCount: 0
        property int clearCompletedCount: 0
        signal transfer_jobs_changed()

        function refresh_transfer_jobs() {}
        function transfer_job_items(id) {
            return detailItems
        }
        function cancel_transfer_job(id) {
            cancelledId = id
        }
        function retry_transfer_job(id) {
            retriedId = id
        }
        function cancel_pending_transfers() {
            cancelPendingCount++
        }
        function clear_completed_transfers() {
            clearCompletedCount++
        }
    }

    QtObject {
        id: darkTheme
        property string surface: "#151820"
        property string surface_alt: "#1c202a"
        property string background: "#101217"
        property string foreground: "#e7e9ee"
        property string muted: "#9299a8"
        property string accent: "#8ab4f8"
        property string danger: "#ff6b6b"
        property string border: "#454b58"
        property string hover: "#303746"
        property string font_family: "sans-serif"
        property string mono_font: "monospace"
        property real font_scale: 1
        property real radius: 8
    }

    TransferManager {
        id: manager
        backend: fakeBackend
        theme: null
    }

    ProgressPanel {
        id: panel
        backend: fakeBackend
        manager: manager
        theme: null
    }

    function init() {
        manager.close()
        manager.theme = null
        panel.theme = null
        panel.dismissedJobId = ""
        fakeBackend.cancelledId = ""
        fakeBackend.retriedId = ""
        fakeBackend.transfer_jobs = [
            {
                id: "11",
                operation: "Copy",
                status: "running",
                currentItem: "/source/nested/file.txt",
                sources: "/source",
                destination: "/destination",
                progress: 0.4,
                completedFiles: 1,
                totalFiles: 2,
                completedFolders: 1,
                totalFolders: 1,
                bytesPerSecond: 2 * 1024 * 1024,
                etaSeconds: -1,
                summary: "",
                error: "",
                omittedItemDetails: 0
            },
            {
                id: "12",
                operation: "Move",
                status: "queued",
                currentItem: "",
                sources: "/other",
                destination: "/destination",
                progress: 0,
                completedFiles: 0,
                totalFiles: 1,
                completedFolders: 0,
                totalFolders: 0,
                bytesPerSecond: 0,
                etaSeconds: -1,
                summary: "",
                error: "",
                omittedItemDetails: 0
            },
            {
                id: "13",
                operation: "Delete",
                status: "failed",
                currentItem: "",
                sources: "/locked",
                destination: "",
                progress: 0.5,
                completedFiles: 1,
                totalFiles: 2,
                completedFolders: 0,
                totalFolders: 0,
                bytesPerSecond: 0,
                etaSeconds: -1,
                summary: "Partial delete",
                error: "Permission denied",
                omittedItemDetails: 0
            }
        ]
        fakeBackend.detailItems = [
            { name: "file.txt", status: "completed", error: "" },
            { name: "locked.txt", status: "failed", error: "Permission denied" }
        ]
        fakeBackend.transfer_jobs_changed()
    }

    function test_job_sections_and_expansion_use_backend_item_statuses() {
        compare(manager.jobsFor("running").length, 1)
        compare(manager.jobsFor("queued").length, 1)
        compare(manager.jobsFor("failed").length, 1)
        manager.open = true
        manager.toggleDetails(fakeBackend.transfer_jobs[0])
        compare(manager.expandedJobId, "11")
        compare(manager.expandedItems.length, 2)
        compare(manager.expandedItems[1].status, "failed")
    }

    function test_only_one_completed_job_expands_at_a_time() {
        fakeBackend.transfer_jobs = [
            {
                id: "21",
                operation: "Copy",
                status: "completed",
                sources: "/source",
                destination: "/destination",
                summary: "Copied one file",
                error: "",
                omittedItemDetails: 0
            },
            {
                id: "22",
                operation: "Move",
                status: "completed",
                sources: "/other",
                destination: "/destination",
                summary: "Moved one file",
                error: "",
                omittedItemDetails: 0
            },
            {
                id: "23",
                operation: "Delete",
                status: "completed",
                sources: "/third",
                destination: "",
                summary: "Deleted one file",
                error: "",
                omittedItemDetails: 0
            }
        ]
        fakeBackend.transfer_jobs_changed()
        manager.open = true
        wait(0)

        var firstButton = findChild(manager, "transferDetailsButton_21")
        var secondButton = findChild(manager, "transferDetailsButton_22")
        var thirdButton = findChild(manager, "transferDetailsButton_23")
        var firstDetails = findChild(manager, "transferDetailsList_21")
        var secondDetails = findChild(manager, "transferDetailsList_22")
        var thirdDetails = findChild(manager, "transferDetailsList_23")
        var firstCard = findChild(manager, "transferJobCard_21")
        var secondCard = findChild(manager, "transferJobCard_22")
        var thirdCard = findChild(manager, "transferJobCard_23")
        verify(firstButton !== null)
        verify(secondButton !== null)
        verify(thirdButton !== null)
        verify(firstDetails !== null)
        verify(secondDetails !== null)
        verify(thirdDetails !== null)
        verify(firstCard !== null)
        verify(secondCard !== null)
        verify(thirdCard !== null)
        compare(String(firstCard.job.id), "21")
        compare(String(secondCard.job.id), "22")
        compare(String(thirdCard.job.id), "23")

        manager.toggleDetails(fakeBackend.transfer_jobs[0])
        tryCompare(manager, "expandedJobId", "21")
        compare(firstCard.expanded, true)
        compare(firstDetails.count, 2)
        compare(secondDetails.count, 0)
        compare(thirdDetails.count, 0)
        compare(secondCard.expanded, false)
        tryVerify(function() {
            return secondCard.y >= firstCard.y + firstCard.height
        })
        tryVerify(function() {
            return thirdCard.y >= secondCard.y + secondCard.height
        })

        manager.toggleDetails(fakeBackend.transfer_jobs[1])
        tryCompare(manager, "expandedJobId", "22")
        compare(firstCard.expanded, false)
        compare(secondCard.expanded, true)
        compare(firstDetails.count, 0)
        compare(secondDetails.count, 2)
        compare(thirdDetails.count, 0)
        tryVerify(function() {
            return secondCard.y >= firstCard.y + firstCard.height
        })
        tryVerify(function() {
            return thirdCard.y >= secondCard.y + secondCard.height
        })
    }

    function test_routes_show_source_and_destination_for_active_and_completed_jobs() {
        fakeBackend.transfer_jobs = [
            {
                id: "31",
                operation: "Copy",
                status: "running",
                currentItem: "/source/file.txt",
                sources: "/source/file.txt",
                destination: "/destination",
                progress: 0.5,
                completedFiles: 0,
                totalFiles: 1,
                completedFolders: 0,
                totalFolders: 0,
                bytesPerSecond: 0,
                etaSeconds: -1,
                summary: "",
                error: "",
                omittedItemDetails: 0
            },
            {
                id: "32",
                operation: "Copy",
                status: "completed",
                currentItem: "",
                sources: "/source/finished.txt",
                destination: "/destination",
                progress: 1,
                completedFiles: 1,
                totalFiles: 1,
                completedFolders: 0,
                totalFolders: 0,
                bytesPerSecond: 0,
                etaSeconds: -1,
                summary: "Copied one file",
                error: "",
                omittedItemDetails: 0
            }
        ]
        fakeBackend.transfer_jobs_changed()
        manager.open = true
        wait(0)

        var activeRoute = findChild(manager, "transferRoute_31")
        var currentItem = findChild(manager, "transferCurrentItem_31")
        var completedRoute = findChild(manager, "transferRoute_32")
        verify(activeRoute !== null)
        verify(currentItem !== null)
        verify(completedRoute !== null)
        compare(activeRoute.text, "/source/file.txt → /destination")
        compare(currentItem.text, "Current: /source/file.txt")
        compare(completedRoute.text, "/source/finished.txt → /destination")
    }

    function test_job_and_item_sizes_are_shown() {
        fakeBackend.transfer_jobs = [{
            id: "41",
            operation: "Copy",
            status: "running",
            currentItem: "/source/folder",
            sources: "/source/folder",
            destination: "/destination",
            progress: 0.5,
            completedFiles: 1,
            totalFiles: 2,
            completedFolders: 0,
            totalFolders: 1,
            bytesPerSecond: 0,
            etaSeconds: -1,
            sizeKnown: true,
            hasProgressBytes: true,
            completedSize: "512 KiB",
            totalSize: "1.0 MiB",
            summary: "",
            error: "",
            omittedItemDetails: 0
        }]
        fakeBackend.detailItems = [{
            path: "/source/folder",
            name: "folder",
            isDirectory: true,
            size: "1.0 MiB",
            status: "running",
            error: ""
        }]
        fakeBackend.transfer_jobs_changed()
        manager.open = true
        wait(0)

        var size = findChild(manager, "transferSize_41")
        verify(size !== null)
        compare(size.text, "Data: 512 KiB / 1.0 MiB")
        manager.toggleDetails(fakeBackend.transfer_jobs[0])
        wait(0)
        var itemSize = findChild(manager, "transferDetailSize")
        verify(itemSize !== null)
        compare(itemSize.text, "Folder · 1.0 MiB")
    }

    function test_job_start_and_finish_times_are_shown() {
        var startedAt = Date.UTC(2026, 10, 7, 17, 40, 0)
        var finishedAt = Date.UTC(2026, 10, 7, 17, 42, 0)
        fakeBackend.transfer_jobs = [{
            id: "42",
            operation: "Copy",
            status: "completed",
            sources: "/source/file.txt",
            destination: "/destination",
            startedAtMs: startedAt,
            finishedAtMs: finishedAt,
            summary: "Copied one file",
            error: "",
            omittedItemDetails: 0
        }]
        fakeBackend.transfer_jobs_changed()
        manager.open = true
        wait(0)

        var times = findChild(manager, "transferTimes_42")
        verify(times !== null)
        verify(times.visible)
        verify(times.text.indexOf("Started at "
                                  + new Date(startedAt).toLocaleTimeString()) >= 0)
        verify(times.text.indexOf("Finished at "
                                  + new Date(finishedAt).toLocaleTimeString()) >= 0)
    }

    function test_job_list_wheel_scroll_is_smoothed() {
        var jobs = []
        for (var index = 0; index < 20; index++) {
            jobs.push({
                id: String(100 + index),
                operation: "Copy",
                status: "completed",
                sources: "/source",
                destination: "/destination",
                summary: "Copied one file",
                error: "",
                omittedItemDetails: 0
            })
        }
        fakeBackend.transfer_jobs = jobs
        fakeBackend.transfer_jobs_changed()
        manager.open = true
        wait(0)

        var flickable = findChild(manager, "transferJobsFlickable")
        verify(flickable !== null)
        verify(flickable.contentHeight > flickable.height)

        manager.scrollJobsByWheel(-120)
        wait(45)
        verify(flickable.contentY > 0)
        verify(flickable.contentY < manager.jobsWheelTargetY)
        tryVerify(function() {
            return Math.abs(flickable.contentY - manager.jobsWheelTargetY) < 1
        }, 300)
    }

    function test_compact_progress_indicator_opens_manager() {
        compare(panel.jobs.length, 3)
        compare(panel.hasQueueJobs, true)
        panel.openManager()
        compare(manager.open, true)
    }

    function test_transfer_manager_dark_theme_has_visible_surface_and_close_button() {
        manager.theme = darkTheme
        panel.theme = darkTheme
        var dialogPanel = findChild(manager, "transferManagerPanel")
        var closeButton = findChild(manager, "closeTransferManagerButton")
        verify(dialogPanel !== null)
        verify(closeButton !== null)
        compare(dialogPanel.color, darkTheme.surface_alt)
        compare(closeButton.color, darkTheme.surface)
        compare(findChild(closeButton, "closeTransferManagerGlyph").color, darkTheme.foreground)

        fakeBackend.transfer_jobs = [{
            id: "14",
            operation: "Copy",
            status: "completed",
            currentItem: "",
            sources: "/source",
            destination: "/destination",
            progress: 1,
            completedFiles: 1,
            totalFiles: 1,
            bytesPerSecond: 0,
            summary: "Copied 1 file",
            error: ""
        }]
        fakeBackend.transfer_jobs_changed()
        var clearCompletedButton = findChild(manager, "clearCompletedTransfersButton")
        verify(clearCompletedButton !== null)
        verify(clearCompletedButton.enabled)
        compare(clearCompletedButton.contentItem.color, darkTheme.foreground)
        compare(findChild(panel, "progressPanelActionBackground").color, darkTheme.surface_alt)
    }

    function test_compact_progress_shows_percentage_and_speed() {
        var metrics = findChild(panel, "compactTransferMetrics")
        verify(metrics.text.indexOf("40%") >= 0)
        verify(metrics.text.indexOf("2.0 MB/s") >= 0)
    }

    function test_compact_progress_shows_transferred_size() {
        fakeBackend.transfer_jobs = [{
            id: "44",
            operation: "Copy",
            status: "running",
            progress: 0.5,
            completedFiles: 1,
            totalFiles: 2,
            completedFolders: 0,
            totalFolders: 0,
            sizeKnown: true,
            hasProgressBytes: true,
            completedSize: "512 KiB",
            totalSize: "1.0 MiB",
            bytesPerSecond: 0,
            etaSeconds: -1,
            summary: "",
            error: ""
        }]
        fakeBackend.transfer_jobs_changed()

        var metrics = findChild(panel, "compactTransferMetrics")
        verify(metrics.text.indexOf("50%") >= 0)
        verify(metrics.text.indexOf("512 KiB / 1.0 MiB") >= 0)
    }

    function test_compact_progress_shows_eta_when_running() {
        fakeBackend.transfer_jobs = [{
            id: "15",
            operation: "Copy",
            status: "running",
            currentItem: "/source/video.mp4",
            sources: "/source",
            destination: "/destination",
            progress: 0.5,
            completedFiles: 2,
            totalFiles: 4,
            completedFolders: 0,
            totalFolders: 0,
            bytesPerSecond: 10 * 1024 * 1024,
            etaSeconds: 85,
            summary: "",
            error: "",
            omittedItemDetails: 0
        }]
        fakeBackend.transfer_jobs_changed()
        var metrics = findChild(panel, "compactTransferMetrics")
        verify(metrics.text.indexOf("1m 25s left") >= 0)
    }

    function test_completed_compact_indicator_can_be_dismissed_without_clearing_history() {
        fakeBackend.transfer_jobs = [{
            id: "14",
            operation: "Copy",
            status: "completed",
            currentItem: "",
            sources: "/source",
            destination: "/destination",
            progress: 1,
            completedFiles: 1,
            totalFiles: 1,
            bytesPerSecond: 0,
            summary: "Copied 1 file",
            error: ""
        }]
        fakeBackend.transfer_jobs_changed()
        compare(panel.jobs.length, 1)
        compare(panel.terminalJob, true)
        compare(panel.dismissedJobId, "")
        compare(panel.activeJob.id, "14")
        verify(findChild(panel, "compactTransferMetrics").text.indexOf("100%") >= 0)
        verify(panel.hasQueueJobs)
        verify(!(panel.terminalJob && panel.dismissedJobId === String(panel.activeJob.id)))
        compare(panel.shouldShow, true)
        panel.dismissCompletedJob()
        compare(panel.shouldShow, false)
        compare(fakeBackend.transfer_jobs.length, 1)
    }

    function test_main_progress_panel_minimizes_and_restores() {
        var minimize = findChild(panel, "progressPanelMinimize")
        verify(minimize !== null)
        verify(findChild(panel, "progressPanelAction") !== null)
        compare(findChild(minimize, "progressPanelMinimizeText").text, "⌄")
        verify(findChild(manager, "minimizeTransferManagerButton") === null)
        panel.minimize()
        compare(panel.minimized, true)
        compare(panel.height, 48)
        verify(panel.width < 220)
        panel.restore()
        compare(panel.minimized, false)
        compare(panel.height, 112)
    }

    function test_completed_action_remains_separate_from_minimize_arrow() {
        fakeBackend.transfer_jobs = [{
            id: "14",
            operation: "Copy",
            status: "completed",
            currentItem: "",
            sources: "/source",
            destination: "/destination",
            progress: 1,
            completedFiles: 1,
            totalFiles: 1,
            bytesPerSecond: 0,
            summary: "Copied 1 file",
            error: ""
        }]
        fakeBackend.transfer_jobs_changed()
        compare(findChild(panel, "progressPanelActionText").text, "×")
    }

    function test_escape_closes_transfer_manager() {
        manager.open = true
        manager.forceActiveFocus()
        keyClick(Qt.Key_Escape)
        tryCompare(manager, "open", false)
    }
}
