import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    name: "PerformanceDialog"
    when: windowShown
    width: 800
    height: 700

    QtObject {
        id: fakeLifecycle
        property bool keep_running_background: true
        property int background_idle_timeout_seconds: 30
        property string startup_behavior: "balanced"
        property int thumbnail_cache_limit_mb: 512
        property string settings_error: ""
        property string cache_clear_status: ""
        property int last_timeout: -1

        function set_keep_running_background(enabled) {
            keep_running_background = enabled
        }
        function set_background_idle_timeout(seconds) {
            last_timeout = seconds
            background_idle_timeout_seconds = seconds
        }
        function set_startup_behavior(value) {
            startup_behavior = value
        }
        function set_thumbnail_cache_limit(megabytes) {
            thumbnail_cache_limit_mb = megabytes
        }
        function clear_thumbnail_cache() {
            cache_clear_status = "Clearing Filem thumbnails…"
        }
        function poll_cache_clear_status() {}
    }

    QtObject {
        id: fakeTheme
        property string mode: "dark"
        property real radius: 8
        property real font_scale: 1
        property string font_family: "sans-serif"
        property string background: "#0f1115"
        property string surface: "#151820"
        property string surface_alt: "#1c202a"
        property string foreground: "#e7e9ee"
        property string muted: "#9299a8"
        property string accent: "#8ab4f8"
        property string border: "#292e3a"
        property string selection: "#24364f"
        property string selection_text: "#f2f5fb"
        property string hover: "#202531"
        property string focus: "#8ab4f8"
        property string danger: "#f28b82"
    }

    PerformanceDialog {
        id: dialog
        theme: fakeTheme
        lifecycle: fakeLifecycle
        open: true
    }

    function test_requested_lifecycle_choices_are_available() {
        compare(dialog.timeoutLabels.join(","), "5 seconds,10 seconds,30 seconds,1 minute,5 minutes,Until logout")
        compare(dialog.startupLabels.length, 3)
        compare(dialog.cacheLimits.join(","), "128,256,512,1024,2048")
        verify(findChild(dialog, "keepRunningBackground") !== null)
        verify(findChild(dialog, "backgroundIdleTimeout") !== null)
        verify(findChild(dialog, "startupBehavior") !== null)
        verify(findChild(dialog, "thumbnailCacheLimit") !== null)
        var selectedCacheOption = findChild(dialog, "thumbnailCacheOption_512")
        verify(selectedCacheOption !== null)
        compare(selectedCacheOption.emphasized, true)
    }

    function test_settings_content_does_not_scroll_horizontally() {
        var scrollView = findChild(dialog, "performanceSettingsScrollView")
        var content = findChild(dialog, "performanceSettingsContent")
        var horizontalBar = findChild(dialog, "performanceSettingsHorizontalScrollBar")
        verify(scrollView !== null)
        verify(content !== null)
        verify(horizontalBar !== null)
        verify(content.width <= scrollView.availableWidth)
        compare(horizontalBar.visible, false)
    }

    function test_settings_are_forwarded_to_persistent_backend() {
        fakeLifecycle.set_keep_running_background(false)
        fakeLifecycle.set_background_idle_timeout(dialog.timeoutSeconds[5])
        fakeLifecycle.set_startup_behavior(dialog.startupValues[1])
        fakeLifecycle.set_thumbnail_cache_limit(dialog.cacheLimits[3])
        compare(fakeLifecycle.keep_running_background, false)
        compare(fakeLifecycle.last_timeout, 0)
        compare(fakeLifecycle.startup_behavior, "fast_reopen")
        compare(fakeLifecycle.thumbnail_cache_limit_mb, 1024)
        compare(dialog.timeoutIndex(), 5)
        compare(dialog.startupIndex(), 1)
        compare(dialog.cacheIndex(), 3)
    }

    function test_clear_cache_has_visible_status() {
        fakeLifecycle.clear_thumbnail_cache()
        compare(fakeLifecycle.cache_clear_status, "Clearing Filem thumbnails…")
    }
}
