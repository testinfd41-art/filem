import QtQuick
import QtTest
import "../../qml/components"

TestCase {
    name: "SearchFilters"
    when: windowShown
    width: 900
    height: 600

    QtObject {
        id: fakeTheme
        property string font_family: "sans-serif"
        property real font_scale: 1
        property real radius: 8
        property color accent: "#8ab4f8"
        property color border: "#292e3a"
        property color danger: "#f28b82"
        property color focus: "#8ab4f8"
        property color foreground: "#e7e9ee"
        property color hover: "#202531"
        property color muted: "#9299a8"
        property color selection: "#24364f"
        property color selection_text: "#f2f5fb"
        property color surface: "#151820"
        property color surface_alt: "#1c202a"
    }

    QtObject {
        id: fakeBackend
        property bool search_recursive: false
        property bool recent_view: false
        property string lastQuery: ""
        property string lastFilters: ""
        property bool cleared: false

        function search_advanced(query, filters, recursive) {
            lastQuery = query
            lastFilters = filters
            cleared = false
        }

        function clear_search() {
            cleared = true
        }

        function set_search_recursive_advanced(query, filters, recursive) {
            search_recursive = recursive
            lastQuery = query
            lastFilters = filters
        }
    }

    LocationBar {
        id: bar
        width: 800
        height: 30
        theme: fakeTheme
        backend: fakeBackend
        searchMode: true
    }

    function test_filter_expression_and_search_dispatch() {
        bar.text = "report"
        bar.filterCategory = "Images"
        bar.filterSize = "Custom"
        bar.filterMinSize = "1M"
        bar.filterMaxSize = "2G"
        bar.filterModified = "Today"
        bar.filterExtension = ".PDF, jpg"
        bar.filterIncludeHidden = true
        bar.filterKind = "Files only"

        compare(bar.filterExpression(),
                "type:image size:>=1M size:<=2G modified:today ext:pdf ext:jpg hidden:true kind:files")
        bar.runSearch()
        compare(fakeBackend.lastQuery, "report")
        compare(fakeBackend.lastFilters, bar.filterExpression())
        verify(!fakeBackend.cleared)
    }

    function test_filter_chips_are_removable_and_clearable() {
        bar.filterCategory = "Videos"
        bar.filterKind = "Folders only"
        compare(bar.activeFilterChips().length, 2)

        bar.removeFilter("category")
        compare(bar.filterCategory, "")
        compare(bar.activeFilterChips().length, 1)

        bar.clearFilters()
        compare(bar.activeFilterChips().length, 0)
        compare(bar.filterExpression(), "")
    }

    function test_invalid_custom_values_are_not_applied() {
        bar.clearFilters()
        bar.filterCategory = "Images"
        bar.filterSize = "Custom"
        bar.filterMinSize = "large"
        bar.filterModified = "Custom range"
        bar.filterDateFrom = "2026-02-31"
        bar.filterDateTo = ""

        compare(bar.filterExpression(), "type:image")
    }

    function test_filter_button_opens_advanced_filter_panel() {
        var popup = findChild(bar, "searchFilterPopup")
        verify(popup !== null)
        var filterButton = findChild(bar, "searchFilterButton")
        var filterClick = findChild(bar, "searchFilterButtonMouseArea")
        verify(findChild(filterButton, "searchFilterIcon") !== null)
        compare(findChild(filterButton, "searchFilterLabel"), null)
        verify(filterClick !== null)
        bar.toggleFilterPopup()
        tryCompare(popup, "visible", true)
        bar.toggleFilterPopup()
        tryCompare(popup, "visible", false)
        bar.toggleFilterPopup()
        tryCompare(popup, "visible", true)
        popup.close()
    }

    function test_subfolder_scope_is_inside_filters_and_preserves_filters() {
        bar.clearFilters()
        fakeBackend.search_recursive = false
        bar.text = "report"
        bar.filterCategory = "Documents"

        var popup = findChild(bar, "searchFilterPopup")
        var scope = findChild(bar, "searchSubfoldersToggle")
        var scroll = findChild(bar, "searchFilterContentScroll")
        verify(popup !== null)
        verify(scope !== null)
        verify(scroll !== null)
        verify(scroll.contentWidth <= scroll.availableWidth)
        popup.open()
        tryCompare(popup, "visible", true)
        var typeOptions = findChild(bar, "searchTypeFilterOptions")
        var typeRepeater = findChild(bar, "searchTypeOptionsRepeater")
        tryVerify(function() {
            var lastOption = typeRepeater.itemAt(6)
            return typeOptions.width <= scroll.availableWidth
                    && lastOption.x + lastOption.width <= typeOptions.width
                    && lastOption.y > typeRepeater.itemAt(0).y
        })
        mouseClick(scope, scope.width / 2, scope.height / 2)
        compare(fakeBackend.search_recursive, true)
        compare(fakeBackend.lastQuery, "report")
        compare(fakeBackend.lastFilters, "type:document")
        verify(bar.activeFilterChips().some(function(chip) {
            return chip.key === "subfolders"
        }))
        popup.close()
    }
}
