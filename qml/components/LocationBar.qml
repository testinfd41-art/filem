import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root

    property var theme
    property var backend
    property bool searchMode: false
    property alias text: input.text
    property string filterCategory: ""
    property string filterSize: ""
    property string filterMinSize: ""
    property string filterMaxSize: ""
    property string filterModified: ""
    property string filterDateFrom: ""
    property string filterDateTo: ""
    property string filterExtension: ""
    property bool filterIncludeHidden: false
    property string filterKind: ""

    signal accepted(string path)
    signal canceled()
    signal moveToList()

    implicitHeight: 30
    radius: theme ? Math.max(4, theme.radius * 0.55) : 6
    color: theme ? theme.surface_alt : "#1c202a"
    border.width: 1
    border.color: input.activeFocus
                  ? (theme ? theme.focus : "#8ab4f8")
                  : (theme ? theme.border : "#292e3a")

    Behavior on border.color {
        ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
    }

    function focusInput() {
        input.forceActiveFocus()
        input.selectAll()
    }

    function runSearch() {
        if (!backend)
            return
        var filters = filterExpression()
        if (input.text.trim() === "" && filters === "")
            backend.clear_search()
        else
            backend.search_advanced(input.text, filters, backend.search_recursive)
    }

    function validSize(value) {
        return /^\d+(\.\d+)?[kKmMgGtT]?$/.test(value.trim())
    }

    function validDate(value) {
        if (value === "")
            return true
        if (!/^\d{4}-\d{2}-\d{2}$/.test(value))
            return false
        var parts = value.split("-")
        var year = Number(parts[0])
        var month = Number(parts[1])
        var day = Number(parts[2])
        if (year < 1970 || year > 9999)
            return false
        var date = new Date(Date.UTC(year, month - 1, day))
        return date.getUTCFullYear() === year
                && date.getUTCMonth() === month - 1
                && date.getUTCDate() === day
    }

    function filterExpression() {
        var tokens = []
        var categories = {
            "Images": "image",
            "Videos": "video",
            "Audio": "audio",
            "Documents": "document",
            "Archives": "archive",
            "Folders": "folder",
            "Text / code": "text"
        }
        if (filterCategory !== "" && categories[filterCategory])
            tokens.push("type:" + categories[filterCategory])
        if (filterSize === "Empty" || filterSize === "Small"
                || filterSize === "Medium" || filterSize === "Large") {
            tokens.push("size:" + filterSize.toLowerCase())
        } else if (filterSize === "Custom") {
            if (filterMinSize.trim() !== "" && validSize(filterMinSize))
                tokens.push("size:>=" + filterMinSize.trim())
            if (filterMaxSize.trim() !== "" && validSize(filterMaxSize))
                tokens.push("size:<=" + filterMaxSize.trim())
        }
        if (filterModified === "Custom range") {
            var from = filterDateFrom.trim()
            var to = filterDateTo.trim()
            if ((from === "" || validDate(from))
                    && (to === "" || validDate(to))
                    && (from === "" || to === "" || from <= to)
                    && (from !== "" || to !== ""))
                tokens.push("modified:" + filterDateFrom.trim() + ".." + filterDateTo.trim())
        } else if (filterModified !== "") {
            var dateModes = {
                "Today": "today",
                "Yesterday": "yesterday",
                "Last 7 days": "last7d",
                "Last 30 days": "last30d"
            }
            if (dateModes[filterModified])
                tokens.push("modified:" + dateModes[filterModified])
        }
        var extensions = filterExtension.toLowerCase().split(/[,;\s]+/)
        for (var i = 0; i < extensions.length; i++) {
            var extension = extensions[i].replace(/^\./, "")
            if (extension !== "")
                tokens.push("ext:" + extension)
        }
        if (filterIncludeHidden)
            tokens.push("hidden:true")
        if (filterKind === "Files only")
            tokens.push("kind:files")
        else if (filterKind === "Folders only")
            tokens.push("kind:folders")
        return tokens.join(" ")
    }

    function activeFilterChips() {
        var chips = []
        if (filterCategory !== "")
            chips.push({ key: "category", label: filterCategory })
        if (filterSize !== "") {
            var sizeLabel = filterSize
            if (filterSize === "Custom")
                sizeLabel += " " + (filterMinSize || "any") + "–" + (filterMaxSize || "any")
            chips.push({ key: "size", label: sizeLabel })
        }
        if (filterModified !== "") {
            var dateLabel = filterModified
            if (filterModified === "Custom range")
                dateLabel += " " + (filterDateFrom || "…") + "–" + (filterDateTo || "…")
            chips.push({ key: "modified", label: dateLabel })
        }
        if (filterExtension.trim() !== "")
            chips.push({ key: "extension", label: "Extension: " + filterExtension })
        if (filterIncludeHidden)
            chips.push({ key: "hidden", label: "Hidden files" })
        if (filterKind !== "")
            chips.push({ key: "kind", label: filterKind })
        if (backend && !backend.recent_view && backend.search_recursive)
            chips.push({ key: "subfolders", label: "Subfolders" })
        return chips
    }

    function scheduleSearch() {
        if (searchMode && backend)
            debounce.restart()
    }

    function removeFilter(key) {
        if (key === "category")
            filterCategory = ""
        else if (key === "size") {
            filterSize = ""
            filterMinSize = ""
            filterMaxSize = ""
        } else if (key === "modified") {
            filterModified = ""
            filterDateFrom = ""
            filterDateTo = ""
        } else if (key === "extension")
            filterExtension = ""
        else if (key === "hidden")
            filterIncludeHidden = false
        else if (key === "kind")
            filterKind = ""
        else if (key === "subfolders") {
            setSearchRecursive(false)
            return
        }
        scheduleSearch()
    }

    function setSearchRecursive(enabled) {
        if (!backend || backend.search_recursive === enabled)
            return
        debounce.stop()
        backend.set_search_recursive_advanced(input.text, filterExpression(), enabled)
    }

    function toggleFilterPopup() {
        if (filterPopup.visible)
            filterPopup.close()
        else
            filterPopup.open()
    }

    function clearFilters() {
        var clearSubfolders = backend && backend.search_recursive
        filterCategory = ""
        filterSize = ""
        filterMinSize = ""
        filterMaxSize = ""
        filterModified = ""
        filterDateFrom = ""
        filterDateTo = ""
        filterExtension = ""
        filterIncludeHidden = false
        filterKind = ""
        if (clearSubfolders)
            setSearchRecursive(false)
        else
            scheduleSearch()
    }

    onSearchModeChanged: {
        if (!searchMode)
            filterPopup.close()
    }

    // Three adjustable tracks keep the filter control legible at icon size.
    component FilterIcon: Item {
        id: filterIcon
        property color stroke: "#9299a8"
        width: 18
        height: 18
        Canvas {
            id: filterIconCanvas
            anchors.fill: parent
            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                ctx.strokeStyle = filterIcon.stroke
                ctx.lineWidth = 1.6
                ctx.lineCap = "round"
                ctx.beginPath()
                ctx.moveTo(2.5, 4)
                ctx.lineTo(6, 4)
                ctx.moveTo(10, 4)
                ctx.lineTo(15.5, 4)
                ctx.moveTo(2.5, 9)
                ctx.lineTo(9, 9)
                ctx.moveTo(13, 9)
                ctx.lineTo(15.5, 9)
                ctx.moveTo(2.5, 14)
                ctx.lineTo(4, 14)
                ctx.moveTo(8, 14)
                ctx.lineTo(15.5, 14)
                ctx.stroke()
                ctx.beginPath()
                ctx.arc(8, 4, 2, 0, Math.PI * 2)
                ctx.arc(11, 9, 2, 0, Math.PI * 2)
                ctx.arc(6, 14, 2, 0, Math.PI * 2)
                ctx.stroke()
            }
            Component.onCompleted: requestPaint()
        }

        onStrokeChanged: filterIconCanvas.requestPaint()
    }

    // Rounded selection control used by each filter group.
    component FilterPill: Rectangle {
        id: pill
        property string label: ""
        property bool selected: false
        signal choose()
        height: 24
        width: pillText.implicitWidth + 18
        radius: height / 2
        color: pill.selected
               ? Qt.rgba(theme ? theme.accent.r : 0.54,
                         theme ? theme.accent.g : 0.71,
                         theme ? theme.accent.b : 0.98, 0.13)
               : (pillMouse.containsMouse
                  ? (theme ? theme.hover : "#202531") : "transparent")
        border.width: 1
        border.color: pill.selected
                      ? Qt.rgba(theme ? theme.accent.r : 0.54,
                                theme ? theme.accent.g : 0.71,
                                theme ? theme.accent.b : 0.98, 0.55)
                      : "transparent"

        Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
        Behavior on border.color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }

        Text {
            id: pillText
            anchors.centerIn: parent
            text: pill.label
            color: pill.selected
                   ? (theme ? theme.accent : "#8ab4f8")
                   : (theme ? theme.foreground : "#e7e9ee")
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 10.5 * (theme ? theme.font_scale : 1)
        }

        MouseArea {
            id: pillMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            Accessible.role: Accessible.CheckBox
            Accessible.name: pill.label
            Accessible.checked: pill.selected
            onClicked: pill.choose()
        }
    }

    // Small uppercase section marker inside the filter panel.
    component FilterSectionLabel: Text {
        font.family: theme ? theme.font_family : "sans-serif"
        font.pixelSize: 9.5 * (theme ? theme.font_scale : 1)
        font.weight: Font.DemiBold
        font.capitalization: Font.AllUppercase
        color: theme ? theme.muted : "#9299a8"
    }

    // Text field used by the panel sub-rows; mirrors ModalDialog inputs.
    component FilterField: Rectangle {
        id: field
        property alias text: fieldInput.text
        property string placeholder: ""
        signal edited(string value)
        height: 28
        radius: theme ? Math.max(4, theme.radius * 0.5) : 6
        color: theme ? theme.surface_alt : "#1c202a"
        border.width: 1
        border.color: fieldInput.activeFocus
                      ? ((theme ? theme.focus : "#8ab4f8") || "#8ab4f8")
                      : (theme ? theme.border : "#292e3a")

        Behavior on border.color {
            ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
        }

        TextInput {
            id: fieldInput
            anchors.fill: parent
            anchors.leftMargin: 9
            anchors.rightMargin: 9
            verticalAlignment: TextInput.AlignVCenter
            color: theme ? theme.foreground : "#e7e9ee"
            selectionColor: theme ? theme.selection : "#24364f"
            selectedTextColor: theme ? theme.selection_text : "#f2f5fb"
            font.family: theme ? theme.font_family : "sans-serif"
            font.pixelSize: 12 * (theme ? theme.font_scale : 1)
            selectByMouse: true
            clip: true
            onTextEdited: field.edited(text)
        }

        Text {
            anchors.left: fieldInput.left
            anchors.right: fieldInput.right
            anchors.verticalCenter: fieldInput.verticalCenter
            visible: fieldInput.text === ""
            text: field.placeholder
            color: theme ? theme.muted : "#9299a8"
            opacity: 0.75
            font: fieldInput.font
            elide: Text.ElideRight
        }
    }

    // Error note under custom value rows.
    component FilterErrorNote: Text {
        color: (theme ? theme.danger : "#f28b82") || "#f28b82"
        font.family: theme ? theme.font_family : "sans-serif"
        font.pixelSize: 10 * (theme ? theme.font_scale : 1)
    }

    ToolbarGlyph {
        id: modeIcon
        anchors.left: parent.left
        anchors.leftMargin: 10
        anchors.verticalCenter: parent.verticalCenter
        name: root.searchMode ? "search" : "location"
        color: root.searchMode
               ? (theme ? theme.accent : "#8ab4f8")
               : (theme ? theme.muted : "#9299a8")
    }

    TextInput {
        id: input
        anchors.fill: parent
        anchors.leftMargin: 36
        anchors.rightMargin: root.searchMode ? 48 : 10
        verticalAlignment: TextInput.AlignVCenter
        color: theme ? theme.foreground : "#e7e9ee"
        selectionColor: theme ? theme.selection : "#24364f"
        selectedTextColor: theme ? theme.selection_text : "#f2f5fb"
        font.family: theme ? theme.font_family : "sans-serif"
        font.pixelSize: 13 * (theme ? theme.font_scale : 1)
        selectByMouse: true
        clip: true

        onAccepted: {
            debounce.stop()
            root.accepted(text)
        }
        onTextChanged: {
            if (root.searchMode) {
                if (text.trim() === "") {
                    if (root.filterExpression() === "") {
                        debounce.stop()
                        if (root.backend)
                            root.backend.clear_search()
                    } else {
                        debounce.restart()
                    }
                } else {
                    debounce.restart()
                }
            }
        }

        Keys.onEscapePressed: function(event) {
            root.canceled()
            event.accepted = true
        }

        Keys.onDownPressed: function(event) {
            if (root.searchMode)
                root.moveToList()
            event.accepted = root.searchMode
        }

        Text {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: input.text === ""
            text: root.searchMode
                  ? (root.backend && root.backend.recent_view
                     ? "Search recent files: name, ext:pdf, type:image, tag:red\u2026"
                     : "Search this folder: name, ext:pdf, type:image, tag:red\u2026")
                  : "Enter a file or folder location"
            color: theme ? theme.muted : "#9299a8"
            opacity: 0.75
            font: input.font
            elide: Text.ElideRight
        }
    }

    Item {
        id: filterButton
        objectName: "searchFilterButton"
        visible: root.searchMode
        z: 2
        anchors.right: parent.right
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        width: 34
        height: 26

        Rectangle {
            anchors.fill: parent
            radius: 8
            color: filterPopup.visible || root.activeFilterChips().length > 0
                   ? Qt.rgba(theme ? theme.accent.r : 0.54,
                             theme ? theme.accent.g : 0.71,
                             theme ? theme.accent.b : 0.98, 0.14)
                   : (filterMouse.containsMouse
                      ? (theme ? theme.hover : "#202531") : "transparent")
            border.width: 1
            border.color: filterPopup.visible || root.activeFilterChips().length > 0
                          ? Qt.rgba(theme ? theme.accent.r : 0.54,
                                    theme ? theme.accent.g : 0.71,
                                    theme ? theme.accent.b : 0.98, 0.55)
                          : (theme ? theme.border : "#292e3a")

            Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
            Behavior on border.color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }

            FilterIcon {
                objectName: "searchFilterIcon"
                anchors.centerIn: parent
                stroke: filterPopup.visible || root.activeFilterChips().length > 0
                        ? (theme ? theme.accent : "#8ab4f8")
                        : (theme ? theme.muted : "#9299a8")
            }

            Rectangle {
                visible: root.activeFilterChips().length > 0
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.rightMargin: -3
                anchors.topMargin: -3
                width: 13
                height: 13
                radius: 7
                color: theme ? theme.accent : "#8ab4f8"

                Text {
                    anchors.centerIn: parent
                    text: root.activeFilterChips().length
                    color: theme ? theme.surface : "#151820"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 8 * (theme ? theme.font_scale : 1)
                    font.weight: Font.DemiBold
                }
            }
        }

        MouseArea {
            id: filterMouse
            objectName: "searchFilterButtonMouseArea"
            anchors.fill: parent
            z: 1
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            Accessible.role: Accessible.Button
            Accessible.name: root.activeFilterChips().length > 0
                             ? "Advanced search filters, "
                               + root.activeFilterChips().length + " active"
                             : "Advanced search filters"
            onClicked: root.toggleFilterPopup()
        }
    }

    Popup {
        id: filterPopup
        objectName: "searchFilterPopup"
        x: Math.max(0, root.width - width)
        y: root.height + 6
        width: 388
        height: 500
        padding: 14
        modal: false
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        background: Rectangle {
            color: theme ? theme.surface : "#151820"
            border.width: 1
            border.color: theme ? theme.border : "#292e3a"
            radius: theme ? Math.max(6, theme.radius) : 10
        }

        contentItem: ColumnLayout {
            spacing: 10

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                FilterIcon {
                    objectName: "searchFilterPanelIcon"
                    Layout.alignment: Qt.AlignVCenter
                    stroke: theme ? theme.accent : "#8ab4f8"
                }

                Text {
                    Layout.alignment: Qt.AlignVCenter
                    text: "Search filters"
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                    font.weight: Font.DemiBold
                }

                Item { Layout.fillWidth: true }

                Rectangle {
                    id: clearAllButton
                    Layout.alignment: Qt.AlignVCenter
                    width: clearAllText.implicitWidth + 16
                    height: 24
                    radius: 6
                    visible: root.activeFilterChips().length > 0
                    color: clearAllMouse.containsMouse
                           ? (theme ? theme.hover : "#202531") : "transparent"

                    Text {
                        id: clearAllText
                        anchors.centerIn: parent
                        text: "Clear all"
                        color: clearAllMouse.containsMouse
                               ? ((theme ? theme.danger : "#f28b82") || "#f28b82")
                               : (theme ? theme.muted : "#9299a8")
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                    }

                    MouseArea {
                        id: clearAllMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.clearFilters()
                    }
                }
            }

            Flow {
                id: filterChips
                Layout.fillWidth: true
                spacing: 6

                Repeater {
                    model: root.activeFilterChips()
                    delegate: Rectangle {
                        id: activeChip
                        required property var modelData
                        height: 22
                        width: activeChipRow.implicitWidth + 14
                        radius: height / 2
                        color: activeChipMouse.containsMouse
                               ? (theme ? theme.hover : "#202531")
                               : (theme ? theme.surface_alt : "#1c202a")
                        border.width: 1
                        border.color: theme ? theme.border : "#292e3a"

                        Row {
                            id: activeChipRow
                            anchors.centerIn: parent
                            spacing: 6

                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: activeChip.modelData.label
                                color: theme ? theme.foreground : "#e7e9ee"
                                font.family: theme ? theme.font_family : "sans-serif"
                                font.pixelSize: 10.5 * (theme ? theme.font_scale : 1)
                            }

                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: "\u00d7"
                                color: activeChipMouse.containsMouse
                                       ? (theme ? theme.foreground : "#e7e9ee")
                                       : (theme ? theme.muted : "#9299a8")
                                font.family: theme ? theme.font_family : "sans-serif"
                                font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                            }
                        }

                        MouseArea {
                            id: activeChipMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            Accessible.role: Accessible.Button
                            Accessible.name: "Remove filter " + activeChip.modelData.label
                            onClicked: root.removeFilter(activeChip.modelData.key)
                        }
                    }
                }

                Text {
                    visible: root.activeFilterChips().length === 0
                    text: "No filters yet — pick types, sizes or dates below."
                    color: theme ? theme.muted : "#9299a8"
                    font.pixelSize: 10.5 * (theme ? theme.font_scale : 1)
                }

            }

            Rectangle {
                objectName: "searchSubfoldersFilter"
                visible: root.backend && !root.backend.recent_view
                Layout.fillWidth: true
                height: 32
                radius: 6
                color: scopePopupMouse.containsMouse
                       ? (theme ? theme.hover : "#202531") : "transparent"
                border.width: 1
                border.color: root.backend && root.backend.search_recursive
                              ? Qt.rgba(theme ? theme.accent.r : 0.54,
                                        theme ? theme.accent.g : 0.71,
                                        theme ? theme.accent.b : 0.98, 0.35)
                              : (theme ? theme.border : "#292e3a")

                Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 10
                    anchors.rightMargin: 10
                    spacing: 10

                    Text {
                        Layout.alignment: Qt.AlignVCenter
                        text: "Search subfolders"
                        color: theme ? theme.foreground : "#e7e9ee"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                    }

                    Item { Layout.fillWidth: true }

                    Rectangle {
                        Layout.alignment: Qt.AlignVCenter
                        width: 25
                        height: 14
                        radius: height / 2
                        color: root.backend && root.backend.search_recursive
                               ? (theme ? theme.accent : "#8ab4f8")
                               : (theme ? theme.border : "#454b58")

                        Behavior on color { ColorAnimation { duration: 140; easing.type: Easing.OutCubic } }

                        Rectangle {
                            anchors.verticalCenter: parent.verticalCenter
                            x: root.backend && root.backend.search_recursive ? parent.width - width - 2 : 2
                            width: 10
                            height: 10
                            radius: height / 2
                            color: theme ? theme.surface : "#151820"

                            Behavior on x { NumberAnimation { duration: 140; easing.type: Easing.OutCubic } }
                        }
                    }
                }

                MouseArea {
                    id: scopePopupMouse
                    objectName: "searchSubfoldersToggle"
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    Accessible.role: Accessible.Switch
                    Accessible.name: "Search subfolders"
                    Accessible.checked: root.backend ? root.backend.search_recursive : false
                    onClicked: root.setSearchRecursive(!root.backend.search_recursive)
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: theme ? theme.border : "#292e3a"
                opacity: 0.7
            }

            ScrollView {
                id: filterContentScroll
                objectName: "searchFilterContentScroll"
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                ColumnLayout {
                    width: filterContentScroll.availableWidth
                    spacing: 12

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 6
                        FilterSectionLabel { text: "Type" }
                        Flow {
                            objectName: "searchTypeFilterOptions"
                            Layout.fillWidth: true
                            spacing: 5
                            layoutDirection: Qt.LeftToRight
                            Repeater {
                                objectName: "searchTypeOptionsRepeater"
                                model: ["Images", "Videos", "Audio",
                                        "Documents", "Archives", "Folders", "Text / code"]
                                delegate: FilterPill {
                                    required property string modelData
                                    label: modelData
                                    selected: root.filterCategory === modelData
                                    onChoose: {
                                        root.filterCategory =
                                                root.filterCategory === modelData ? "" : modelData
                                        root.scheduleSearch()
                                    }
                                }
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 6
                        FilterSectionLabel { text: "Size" }
                        Flow {
                            Layout.fillWidth: true
                            spacing: 5
                            Repeater {
                                model: [["Any", ""], ["Empty", "Empty"], ["Small", "Small"],
                                        ["Medium", "Medium"], ["Large", "Large"], ["Custom", "Custom"]]
                                delegate: FilterPill {
                                    required property var modelData
                                    label: modelData[0]
                                    selected: root.filterSize === modelData[1]
                                              && modelData[1] !== ""
                                    onChoose: {
                                        root.filterSize = modelData[1]
                                        if (modelData[1] === "")
                                            root.filterMinSize = ""
                                        root.scheduleSearch()
                                    }
                                }
                            }
                        }
                        Rectangle {
                            visible: root.filterSize === "Custom"
                            Layout.fillWidth: true
                            height: customSizeInfo.implicitHeight + 16
                            radius: 6
                            color: theme ? theme.surface_alt : "#1c202a"

                            Column {
                                id: customSizeInfo
                                x: 8; y: 8
                                width: parent.width - 16
                                spacing: 6

                                RowLayout {
                                    width: parent.width
                                    spacing: 6
                                    FilterField {
                                        Layout.fillWidth: true
                                        placeholder: "Min (e.g. 10M)"
                                        text: root.filterMinSize
                                        onEdited: {
                                            root.filterMinSize = value
                                            root.scheduleSearch()
                                        }
                                    }
                                    FilterField {
                                        Layout.fillWidth: true
                                        placeholder: "Max (e.g. 2G)"
                                        text: root.filterMaxSize
                                        onEdited: {
                                            root.filterMaxSize = value
                                            root.scheduleSearch()
                                        }
                                    }
                                }

                                FilterErrorNote {
                                    visible: (root.filterMinSize.trim() !== ""
                                              && !root.validSize(root.filterMinSize))
                                             || (root.filterMaxSize.trim() !== ""
                                                 && !root.validSize(root.filterMaxSize))
                                    text: "Enter a size such as 512K, 10M, or 2G."
                                }
                            }
                        }
                        Text {
                            visible: root.filterSize !== "Custom"
                            Layout.fillWidth: true
                            text: "Empty, ~1 MiB, 1\u2013100 MiB, over 100 MiB"
                            color: theme ? theme.muted : "#9299a8"
                            opacity: 0.8
                            font.family: theme ? theme.font_family : "sans-serif"
                            font.pixelSize: 9.5 * (theme ? theme.font_scale : 1)
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 6
                        FilterSectionLabel { text: "Modified" }
                        Flow {
                            Layout.fillWidth: true
                            spacing: 5
                            Repeater {
                                model: [["Any date", ""], ["Today", "Today"], ["Yesterday", "Yesterday"],
                                        ["Last 7 days", "Last 7 days"], ["Last 30 days", "Last 30 days"],
                                        ["Custom range", "Custom range"]]
                                delegate: FilterPill {
                                    required property var modelData
                                    label: modelData[0]
                                    selected: root.filterModified === modelData[1]
                                              && modelData[1] !== ""
                                    onChoose: {
                                        root.filterModified = modelData[1]
                                        if (modelData[1] !== "Custom range") {
                                            root.filterDateFrom = ""
                                            root.filterDateTo = ""
                                        }
                                        root.scheduleSearch()
                                    }
                                }
                            }
                        }
                        Rectangle {
                            visible: root.filterModified === "Custom range"
                            Layout.fillWidth: true
                            height: customRangeInfo.implicitHeight + 16
                            radius: 6
                            color: theme ? theme.surface_alt : "#1c202a"

                            Column {
                                id: customRangeInfo
                                x: 8; y: 8
                                width: parent.width - 16
                                spacing: 6

                                RowLayout {
                                    width: parent.width
                                    spacing: 6
                                    FilterField {
                                        Layout.fillWidth: true
                                        placeholder: "From YYYY-MM-DD"
                                        text: root.filterDateFrom
                                        onEdited: {
                                            root.filterDateFrom = value
                                            root.scheduleSearch()
                                        }
                                    }
                                    FilterField {
                                        Layout.fillWidth: true
                                        placeholder: "To YYYY-MM-DD"
                                        text: root.filterDateTo
                                        onEdited: {
                                            root.filterDateTo = value
                                            root.scheduleSearch()
                                        }
                                    }
                                }

                                FilterErrorNote {
                                    visible: (root.filterDateFrom.trim() !== ""
                                              && !root.validDate(root.filterDateFrom.trim()))
                                             || (root.filterDateTo.trim() !== ""
                                                 && !root.validDate(root.filterDateTo.trim()))
                                             || (root.filterDateFrom.trim() !== ""
                                                 && root.filterDateTo.trim() !== ""
                                                 && root.filterDateFrom > root.filterDateTo)
                                    text: "Use valid dates (YYYY-MM-DD) and an ordered range."
                                }
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 6
                        FilterSectionLabel { text: "Extension" }
                        FilterField {
                            Layout.fillWidth: true
                            placeholder: "pdf, jpg, \u2026"
                            text: root.filterExtension
                            onEdited: {
                                root.filterExtension = value
                                root.scheduleSearch()
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 6
                        FilterSectionLabel { text: "Show" }
                        Flow {
                            Layout.fillWidth: true
                            spacing: 5
                            Repeater {
                                model: [["Files and folders", ""], ["Files only", "Files only"],
                                        ["Folders only", "Folders only"]]
                                delegate: FilterPill {
                                    required property var modelData
                                    label: modelData[0]
                                    selected: root.filterKind === modelData[1]
                                              && modelData[1] !== ""
                                    onChoose: {
                                        root.filterKind = root.filterKind === modelData[1]
                                                ? "" : modelData[1]
                                        root.scheduleSearch()
                                    }
                                }
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        height: 32
                        radius: 6
                        color: hiddenToggleMouse.containsMouse
                               ? (theme ? theme.hover : "#202531") : "transparent"
                        border.width: 1
                        border.color: root.filterIncludeHidden
                                      ? Qt.rgba(theme ? theme.accent.r : 0.54,
                                                theme ? theme.accent.g : 0.71,
                                                theme ? theme.accent.b : 0.98, 0.35)
                                      : (theme ? theme.border : "#292e3a")

                        Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }

                        Row {
                            anchors.fill: parent
                            anchors.leftMargin: 10
                            anchors.rightMargin: 10

                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: "Include hidden files"
                                color: theme ? theme.foreground : "#e7e9ee"
                                font.family: theme ? theme.font_family : "sans-serif"
                                font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                            }

                            Rectangle {
                                id: hiddenSwitchTrack
                                anchors.verticalCenter: parent.verticalCenter
                                x: parent.width - width
                                width: 25
                                height: 14
                                radius: height / 2
                                color: root.filterIncludeHidden
                                       ? (theme ? theme.accent : "#8ab4f8")
                                       : (theme ? theme.border : "#454b58")

                                Behavior on color { ColorAnimation { duration: 140; easing.type: Easing.OutCubic } }

                                Rectangle {
                                    anchors.verticalCenter: parent.verticalCenter
                                    x: root.filterIncludeHidden
                                       ? parent.width - width - 2 : 2
                                    width: 10
                                    height: 10
                                    radius: height / 2
                                    color: theme ? theme.surface : "#151820"

                                    Behavior on x {
                                        NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
                                    }
                                }
                            }
                        }

                        MouseArea {
                            id: hiddenToggleMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            Accessible.role: Accessible.Switch
                            Accessible.name: "Include hidden files"
                            Accessible.checked: root.filterIncludeHidden
                            onClicked: {
                                root.filterIncludeHidden = !root.filterIncludeHidden
                                root.scheduleSearch()
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: "Filters combine with your text search \u2014 names, tags and raw queries still apply."
                        color: theme ? theme.muted : "#9299a8"
                        opacity: 0.8
                        wrapMode: Text.WordWrap
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 9.5 * (theme ? theme.font_scale : 1)
                    }
                }
            }
        }
    }

    Timer {
        id: debounce
        interval: 220
        repeat: false
        onTriggered: root.runSearch()
    }

}
