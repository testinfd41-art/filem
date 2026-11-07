import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls

FocusScope {
    id: root

    anchors.fill: parent
    z: 520
    visible: open || opacity > 0
    opacity: open ? 1 : 0

    property var theme
    property var backend
    property bool open: false
    property bool executing: false
    property bool completed: false
    property var paths: []
    property var rules: []
    property var excludedPaths: []
    property bool numbering: false
    property int numberStart: 1
    property int numberStep: 1
    property int numberPadding: 3
    property string numberPlacement: "suffix"
    property string numberSeparator: "_"
    property string sortOrder: "selection"
    property bool descending: false
    property string templateText: ""
    property string extensionMode: "preserve"
    property string extensionText: ""
    property bool caseInsensitive: false
    property string requestedPlanId: ""
    property bool previewPending: false
    property int editRevision: 0
    property int activePreviewRevision: -1

    signal dismissed()

    readonly property var operationTypes: [
        { label: "Find and replace", value: "replace" },
        { label: "Add prefix", value: "prefix" },
        { label: "Add suffix", value: "suffix" },
        { label: "Remove text", value: "removeText" },
        { label: "Remove character range", value: "removeRange" },
        { label: "Uppercase", value: "upper" },
        { label: "Lowercase", value: "lower" },
        { label: "Title Case", value: "title" },
        { label: "Clean characters", value: "clean" },
        { label: "Normalize whitespace", value: "normalizeWhitespace" },
        { label: "Replace spaces", value: "replaceSpaces" }
    ]

    readonly property int selectedCount: backend ? backend.batch_preview_rows.length : 0
    readonly property int affectedCount: countStatus("Ready")
    readonly property int unchangedCount: countStatus("Unchanged")
    readonly property int excludedCount: countStatus("Excluded")
    readonly property int conflictCount: countStatus("Conflict")
    readonly property int invalidCount: countStatus("Invalid")
    readonly property bool canExecute: !executing && !completed && !previewBusy
                                      && !previewPending
                                      && affectedCount > 0
                                      && conflictCount === 0 && invalidCount === 0
    readonly property bool previewBusy: backend ? backend.batch_preview_busy : false

    function countStatus(status) {
        if (!backend)
            return 0
        var count = 0
        var rows = backend.batch_preview_rows
        for (var i = 0; i < rows.length; i++) {
            if (rows[i].status === status)
                count++
        }
        return count
    }

    function openForPaths(selectedPaths) {
        paths = selectedPaths.slice()
        rules = []
        excludedPaths = []
        numbering = false
        numberStart = 1
        numberStep = 1
        numberPadding = 3
        numberPlacement = "suffix"
        numberSeparator = "_"
        sortOrder = "selection"
        descending = false
        templateText = ""
        extensionMode = "preserve"
        extensionText = ""
        caseInsensitive = false
        executing = false
        completed = false
        open = true
        forceActiveFocus()
        Qt.callLater(function() {
            if (root.open)
                root.forceActiveFocus()
        })
        schedulePreview()
    }

    function close() {
        if (!open || executing)
            return
        open = false
        backend.clear_batch_preview()
        dismissed()
    }

    function escapeHtml(text) {
        return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;")
                            .replace(/>/g, "&gt;").replace(/"/g, "&quot;")
    }

    function highlighted(row, proposed) {
        if (row.status === "Unchanged")
            return escapeHtml(proposed ? row.proposed : row.original)
        if (proposed) {
            return escapeHtml(row.newPrefix) + "<font color=\"" + (theme ? theme.accent : "#8ab4f8")
                    + "\"><b>" + escapeHtml(row.newChanged) + "</b></font>"
                    + escapeHtml(row.newSuffix)
        }
        return escapeHtml(row.oldPrefix) + "<font color=\"" + (theme ? theme.muted : "#9299a8")
                + "\"><s>" + escapeHtml(row.oldChanged) + "</s></font>"
                + escapeHtml(row.oldSuffix)
    }

    function updateRule(index, key, value) {
        var updated = rules.slice()
        var previous = updated[index]
        var next = {}
        for (var property in previous)
            next[property] = previous[property]
        next[key] = value
        updated[index] = next
        rules = updated
        schedulePreview()
    }

    function addRule() {
        rules = rules.concat([{
            type: "replace",
            find: "",
            replace: "",
            text: "",
            separator: "_",
            caseSensitive: true,
            wholeWord: false,
            regex: false,
            includeExtension: false,
            start: 0,
            length: 1
        }])
        schedulePreview()
    }

    function removeRule(index) {
        var updated = rules.slice()
        updated.splice(index, 1)
        rules = updated
        schedulePreview()
    }

    function toggleIncluded(path) {
        var updated = excludedPaths.slice()
        var index = updated.indexOf(path)
        if (index >= 0)
            updated.splice(index, 1)
        else
            updated.push(path)
        excludedPaths = updated
        schedulePreview()
    }

    function configuration() {
        return JSON.stringify({
            rules: rules,
            template: templateText,
            extensionMode: extensionMode,
            extension: extensionText,
            numbering: numbering,
            start: numberStart,
            step: numberStep,
            padding: numberPadding,
            numberPlacement: numberPlacement,
            numberSeparator: numberSeparator,
            sort: sortOrder,
            descending: descending,
            caseInsensitive: caseInsensitive
        })
    }

    function schedulePreview() {
        if (open && !executing && !completed) {
            editRevision++
            previewPending = true
            previewTimer.restart()
        }
    }

    function requestPreview() {
        if (!backend || paths.length === 0)
            return
        activePreviewRevision = editRevision
        backend.request_batch_preview(paths, configuration(), excludedPaths)
        requestedPlanId = backend.batch_preview_id
        previewPending = backend.batch_preview_busy
    }

    function execute() {
        if (!canExecute || !backend)
            return
        executing = true
        if (!backend.execute_batch_rename(requestedPlanId))
            executing = false
    }

    function typeIndex(value) {
        for (var i = 0; i < operationTypes.length; i++) {
            if (operationTypes[i].value === value)
                return i
        }
        return 0
    }

    function setOption(name, value) {
        if (name === "numbering")
            numbering = value
        else if (name === "numberStart")
            numberStart = value
        else if (name === "numberStep")
            numberStep = value
        else if (name === "numberPadding")
            numberPadding = value
        else if (name === "numberSeparator")
            numberSeparator = value
        else if (name === "numberPlacement")
            numberPlacement = value
        else if (name === "sortOrder")
            sortOrder = value
        else if (name === "descending")
            descending = value
        else if (name === "templateText")
            templateText = value
        else if (name === "extensionMode")
            extensionMode = value
        else if (name === "extensionText")
            extensionText = value
        else if (name === "caseInsensitive")
            caseInsensitive = value
        schedulePreview()
    }

    Behavior on opacity {
        NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
    }

    Keys.onEscapePressed: function(event) {
        if (root.executing)
            backend.cancel_operation()
        else
            root.close()
        event.accepted = true
    }

    Timer {
        id: previewTimer
        interval: 90
        repeat: false
        onTriggered: root.requestPreview()
    }

    Connections {
        target: root.backend
        function onBatch_preview_changed() {
            if (root.executing && root.backend.batch_preview_rows.length > 0) {
                var finished = true
                var rows = root.backend.batch_preview_rows
                for (var i = 0; i < rows.length; i++) {
                    if (rows[i].status === "Preview") {
                        finished = false
                        break
                    }
                }
                if (finished) {
                    root.executing = false
                    root.completed = true
                }
            }
            if (root.activePreviewRevision === root.editRevision
                    && root.requestedPlanId === root.backend.batch_preview_id
                    && !root.backend.batch_preview_busy)
                root.previewPending = false
        }
    }

    Rectangle {
        anchors.fill: parent
        color: "#000000"
        opacity: 0.62
        MouseArea { anchors.fill: parent }
    }

    Rectangle {
        id: card
        width: Math.min(1040, root.width - 36)
        height: Math.min(760, root.height - 32)
        anchors.centerIn: parent
        radius: theme ? Math.max(8, theme.radius) : 12
        color: theme ? theme.surface : "#151820"
        border.width: 1
        border.color: theme ? theme.border : "#292e3a"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 12

            RowLayout {
                Layout.fillWidth: true
                spacing: 12
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 3
                    Text {
                        text: "Batch Rename"
                        color: theme ? theme.foreground : "#e7e9ee"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 20 * (theme ? theme.font_scale : 1)
                        font.weight: Font.DemiBold
                    }
                    Text {
                        text: root.paths.length + " selected items · Preview changes without touching files"
                        color: theme ? theme.muted : "#9299a8"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    }
                }
                Rectangle {
                    width: 30
                    height: 30
                    radius: 7
                    color: closeMouse.containsMouse ? (theme ? theme.hover : "#202531") : "transparent"
                    Text {
                        anchors.centerIn: parent
                        text: "\u00d7"
                        color: theme ? theme.foreground : "#e7e9ee"
                        font.pixelSize: 20 * (theme ? theme.font_scale : 1)
                    }
                    MouseArea {
                        id: closeMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: root.close()
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: theme ? theme.border : "#292e3a"
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.preferredHeight: 34
                Text {
                    text: "Rename operations"
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.family: theme ? theme.font_family : "sans-serif"
                    font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                    font.weight: Font.DemiBold
                }
                Item { Layout.fillWidth: true }
                Rectangle {
                    width: addRuleLabel.implicitWidth + 24
                    height: 30
                    radius: 6
                    color: addRuleMouse.containsMouse ? (theme ? theme.hover : "#202531")
                                                      : (theme ? theme.surface_alt : "#1c202a")
                    border.width: 1
                    border.color: theme ? theme.border : "#292e3a"
                    Text {
                        id: addRuleLabel
                        anchors.centerIn: parent
                        text: "+ Add rule"
                        color: theme ? theme.foreground : "#e7e9ee"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    }
                    MouseArea {
                        id: addRuleMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        enabled: !root.executing && !root.completed
                        onClicked: root.addRule()
                    }
                }
            }

            Flickable {
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(146, Math.max(50, rulesColumn.implicitHeight))
                contentWidth: width
                contentHeight: rulesColumn.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                Column {
                    id: rulesColumn
                    width: parent.width
                    spacing: 7
                    Repeater {
                        model: root.rules
                        delegate: Rectangle {
                            required property var modelData
                            required property int index
                            width: rulesColumn.width
                            height: ruleContent.implicitHeight + 12
                            radius: 7
                            color: theme ? theme.surface_alt : "#1c202a"
                            border.width: 1
                            border.color: theme ? theme.border : "#292e3a"

                            ColumnLayout {
                                id: ruleContent
                                anchors.fill: parent
                                anchors.margins: 6
                                spacing: 5

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 7
                                    Controls.ComboBox {
                                        Layout.preferredWidth: 190
                                        model: root.operationTypes
                                        textRole: "label"
                                        currentIndex: root.typeIndex(modelData.type)
                                        onActivated: function(index) {
                                            root.updateRule(index, "type",
                                                            root.operationTypes[index].value)
                                        }
                                        contentItem: Text {
                                            leftPadding: 9
                                            text: parent.displayText
                                            color: theme ? theme.foreground : "#e7e9ee"
                                            font.family: theme ? theme.font_family : "sans-serif"
                                            font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                                            verticalAlignment: Text.AlignVCenter
                                            elide: Text.ElideRight
                                        }
                                        background: Rectangle {
                                            radius: 5
                                            color: theme ? theme.surface : "#151820"
                                            border.color: theme ? theme.border : "#292e3a"
                                        }
                                    }
                                    Controls.TextField {
                                        Layout.fillWidth: true
                                        visible: modelData.type === "replace"
                                        placeholderText: "Find"
                                        text: modelData.find
                                        onTextEdited: root.updateRule(index, "find", text)
                                    }
                                    Controls.TextField {
                                        Layout.fillWidth: true
                                        visible: modelData.type === "replace"
                                        placeholderText: "Replace with"
                                        text: modelData.replace
                                        onTextEdited: root.updateRule(index, "replace", text)
                                    }
                                    Controls.TextField {
                                        Layout.fillWidth: true
                                        visible: ["prefix", "suffix", "removeText"].indexOf(modelData.type) >= 0
                                        placeholderText: modelData.type === "prefix" ? "Prefix"
                                                         : modelData.type === "suffix" ? "Suffix" : "Text to remove"
                                        text: modelData.text
                                        onTextEdited: root.updateRule(index, "text", text)
                                    }
                                    Controls.TextField {
                                        Layout.preferredWidth: 95
                                        visible: modelData.type === "replaceSpaces"
                                        placeholderText: "Separator"
                                        text: modelData.separator
                                        onTextEdited: root.updateRule(index, "separator", text)
                                    }
                                    Controls.TextField {
                                        Layout.preferredWidth: 80
                                        visible: modelData.type === "removeRange"
                                        placeholderText: "Start (0-based)"
                                        text: String(modelData.start)
                                        validator: IntValidator { bottom: 0 }
                                        onEditingFinished: root.updateRule(index, "start", Math.max(0, Number(text)))
                                    }
                                    Controls.TextField {
                                        Layout.preferredWidth: 72
                                        visible: modelData.type === "removeRange"
                                        placeholderText: "Length"
                                        text: String(modelData.length)
                                        validator: IntValidator { bottom: 0 }
                                        onEditingFinished: root.updateRule(index, "length", Math.max(0, Number(text)))
                                    }
                                    Rectangle {
                                        width: 26
                                        height: 26
                                        radius: 5
                                        color: removeRuleMouse.containsMouse ? (theme ? theme.hover : "#202531") : "transparent"
                                        Text {
                                            anchors.centerIn: parent
                                            text: "\u2212"
                                            color: theme ? theme.muted : "#9299a8"
                                            font.pixelSize: 18 * (theme ? theme.font_scale : 1)
                                        }
                                        MouseArea {
                                            id: removeRuleMouse
                                            anchors.fill: parent
                                            hoverEnabled: true
                                            onClicked: root.removeRule(index)
                                        }
                                    }
                                }
                                RowLayout {
                                    Layout.fillWidth: true
                                    visible: modelData.type === "replace" || modelData.type === "removeText"
                                    spacing: 12
                                    Controls.CheckBox {
                                        text: "Case sensitive"
                                        checked: modelData.caseSensitive
                                        onToggled: root.updateRule(index, "caseSensitive", checked)
                                    }
                                    Controls.CheckBox {
                                        text: "Whole word"
                                        checked: modelData.wholeWord
                                        onToggled: root.updateRule(index, "wholeWord", checked)
                                    }
                                    Controls.CheckBox {
                                        visible: modelData.type === "replace"
                                        text: "Regular expression"
                                        checked: modelData.regex
                                        onToggled: root.updateRule(index, "regex", checked)
                                    }
                                    Controls.CheckBox {
                                        visible: modelData.type === "replace"
                                        text: "Include extension"
                                        checked: modelData.includeExtension
                                        onToggled: root.updateRule(index, "includeExtension", checked)
                                    }
                                }
                            }
                        }
                    }
                    Text {
                        visible: root.rules.length === 0
                        width: parent.width
                        text: "No transformations yet. Add one or use a template below."
                        color: theme ? theme.muted : "#9299a8"
                        font.family: theme ? theme.font_family : "sans-serif"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                        leftPadding: 4
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: theme ? theme.border : "#292e3a"
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: 12
                rowSpacing: 8

                Text {
                    text: "Template"
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }
                Controls.TextField {
                    Layout.fillWidth: true
                    placeholderText: "{counter:03}_{name}_{date}"
                    text: root.templateText
                    onTextEdited: root.setOption("templateText", text)
                }
                Text {
                    text: "Sequence"
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6
                    Controls.CheckBox {
                        text: "Add numbering"
                        checked: root.numbering
                        onToggled: root.setOption("numbering", checked)
                    }
                    Controls.TextField {
                        Layout.preferredWidth: 64
                        enabled: root.numbering
                        text: String(root.numberStart)
                        validator: IntValidator { bottom: 0 }
                        onEditingFinished: root.setOption("numberStart", Math.max(0, Number(text)))
                    }
                    Controls.TextField {
                        Layout.preferredWidth: 56
                        enabled: root.numbering
                        text: String(root.numberStep)
                        validator: IntValidator { bottom: 1 }
                        onEditingFinished: root.setOption("numberStep", Math.max(1, Number(text)))
                    }
                    Controls.TextField {
                        Layout.preferredWidth: 54
                        enabled: root.numbering
                        text: String(root.numberPadding)
                        validator: IntValidator { bottom: 0; top: 128 }
                        onEditingFinished: root.setOption("numberPadding", Math.max(0, Math.min(128, Number(text))))
                    }
                    Controls.ComboBox {
                        Layout.preferredWidth: 125
                        enabled: root.numbering
                        model: [
                            { label: "Suffix", value: "suffix" },
                            { label: "Prefix", value: "prefix" },
                            { label: "In template", value: "template" }
                        ]
                        textRole: "label"
                        currentIndex: root.numberPlacement === "prefix" ? 1
                                      : root.numberPlacement === "template" ? 2 : 0
                        onActivated: function(index) { root.setOption("numberPlacement", model[index].value) }
                    }
                    Controls.TextField {
                        Layout.preferredWidth: 64
                        enabled: root.numbering && root.numberPlacement !== "template"
                        text: root.numberSeparator
                        onTextEdited: root.setOption("numberSeparator", text)
                    }
                }
                Text {
                    text: "Extension"
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6
                    Controls.ComboBox {
                        Layout.preferredWidth: 150
                        model: [
                            { label: "Preserve", value: "preserve" },
                            { label: "Change to", value: "change" },
                            { label: "Remove", value: "remove" }
                        ]
                        textRole: "label"
                        currentIndex: root.extensionMode === "change" ? 1
                                      : root.extensionMode === "remove" ? 2 : 0
                        onActivated: function(index) { root.setOption("extensionMode", model[index].value) }
                    }
                    Controls.TextField {
                        Layout.preferredWidth: 100
                        visible: root.extensionMode === "change"
                        placeholderText: "png"
                        text: root.extensionText
                        onTextEdited: root.setOption("extensionText", text)
                    }
                }
                Text {
                    text: "Order"
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8
                    Controls.ComboBox {
                        Layout.preferredWidth: 155
                        model: [
                            { label: "Selection order", value: "selection" },
                            { label: "Filename", value: "name" },
                            { label: "Modified date", value: "modified" },
                            { label: "Created date*", value: "created" }
                        ]
                        textRole: "label"
                        currentIndex: root.sortOrder === "name" ? 1
                                      : root.sortOrder === "modified" ? 2
                                      : root.sortOrder === "created" ? 3 : 0
                        onActivated: function(index) { root.setOption("sortOrder", model[index].value) }
                    }
                    Controls.CheckBox {
                        text: "Descending"
                        checked: root.descending
                        onToggled: root.setOption("descending", checked)
                    }
                    Controls.CheckBox {
                        text: "Case-insensitive collisions"
                        checked: root.caseInsensitive
                        onToggled: root.setOption("caseInsensitive", checked)
                    }
                }
                Text {
                    Layout.columnSpan: 2
                    text: "Variables: {name}, {ext}, {index}, {counter:03}, {date}, {modified}, {parent} · *When supported by the filesystem"
                    color: theme ? theme.muted : "#9299a8"
                    font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                    elide: Text.ElideRight
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Text {
                    text: "Preview"
                    color: theme ? theme.foreground : "#e7e9ee"
                    font.pixelSize: 13 * (theme ? theme.font_scale : 1)
                    font.weight: Font.DemiBold
                }
                Item { Layout.fillWidth: true }
                Text {
                    text: root.previewBusy ? "Calculating\u2026"
                          : root.affectedCount + " to rename  ·  " + root.unchangedCount + " unchanged  ·  "
                            + root.excludedCount + " excluded  ·  " + root.conflictCount + " conflicts  ·  "
                            + root.invalidCount + " invalid"
                    color: root.conflictCount + root.invalidCount > 0
                           ? (theme ? theme.danger : "#e06c75")
                           : (theme ? theme.muted : "#9299a8")
                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.minimumHeight: 80
                radius: 7
                color: theme ? theme.surface_alt : "#1c202a"
                border.width: 1
                border.color: theme ? theme.border : "#292e3a"

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 6
                    spacing: 0
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 26
                        Text { Layout.preferredWidth: 28; text: ""; }
                        Text { Layout.fillWidth: true; text: "Original"; color: theme ? theme.muted : "#9299a8"; font.pixelSize: 10 * (theme ? theme.font_scale : 1) }
                        Text { Layout.fillWidth: true; text: "Proposed name"; color: theme ? theme.muted : "#9299a8"; font.pixelSize: 10 * (theme ? theme.font_scale : 1) }
                        Text { Layout.preferredWidth: 126; text: "Status"; color: theme ? theme.muted : "#9299a8"; font.pixelSize: 10 * (theme ? theme.font_scale : 1) }
                    }
                    ListView {
                        id: previewList
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        spacing: 1
                        model: backend ? backend.batch_preview_rows : []
                        delegate: Rectangle {
                            required property var modelData
                            width: previewList.width
                            height: Math.max(27, statusText.implicitHeight + 8)
                            color: "transparent"
                            RowLayout {
                                anchors.fill: parent
                                spacing: 8
                                Rectangle {
                                    Layout.preferredWidth: 18
                                    Layout.preferredHeight: 18
                                    radius: 4
                                    color: modelData.included ? (theme ? theme.selection : "#24364f") : "transparent"
                                    border.width: 1
                                    border.color: theme ? theme.border : "#292e3a"
                                    Text {
                                        anchors.centerIn: parent
                                        visible: modelData.included
                                        text: "\u2713"
                                        color: theme ? theme.selection_text : "#f2f5fb"
                                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                                    }
                                    MouseArea {
                                        anchors.fill: parent
                                        enabled: !root.executing && !root.completed
                                        onClicked: root.toggleIncluded(modelData.path)
                                    }
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: root.highlighted(modelData, false)
                                    textFormat: Text.RichText
                                    color: theme ? theme.foreground : "#e7e9ee"
                                    font.family: theme ? theme.font_family : "sans-serif"
                                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                                    elide: Text.ElideRight
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: root.highlighted(modelData, true)
                                    textFormat: Text.RichText
                                    color: modelData.status === "Conflict" || modelData.status === "Invalid"
                                           ? (theme ? theme.danger : "#e06c75")
                                           : (theme ? theme.foreground : "#e7e9ee")
                                    font.family: theme ? theme.font_family : "sans-serif"
                                    font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                                    elide: Text.ElideRight
                                }
                                ColumnLayout {
                                    Layout.preferredWidth: 126
                                    spacing: 0
                                    Text {
                                        id: statusText
                                        text: modelData.status
                                        color: modelData.status === "Conflict" || modelData.status === "Invalid"
                                               || modelData.status.indexOf("Recovery") >= 0
                                               ? (theme ? theme.danger : "#e06c75")
                                               : modelData.status === "Renamed"
                                                 ? (theme ? theme.accent : "#8ab4f8")
                                                 : (theme ? theme.muted : "#9299a8")
                                        font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                                        elide: Text.ElideRight
                                    }
                                    Text {
                                        visible: modelData.reason !== ""
                                        text: modelData.reason
                                        color: theme ? theme.danger : "#e06c75"
                                        font.pixelSize: 9 * (theme ? theme.font_scale : 1)
                                        elide: Text.ElideRight
                                        Layout.fillWidth: true
                                    }
                                }
                            }
                        }
                        Text {
                            anchors.centerIn: parent
                            visible: previewList.count === 0 && !root.previewBusy
                            text: root.backend && root.backend.batch_preview_error !== ""
                                  ? root.backend.batch_preview_error : "No preview rows"
                            color: theme ? theme.muted : "#9299a8"
                            font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                height: root.executing ? 3 : 0
                visible: root.executing
                radius: 2
                color: theme ? theme.surface_alt : "#1c202a"
                Rectangle {
                    width: parent.width * (backend ? backend.op_progress : 0)
                    height: parent.height
                    radius: 2
                    color: theme ? theme.accent : "#8ab4f8"
                }
            }
            Text {
                visible: root.executing
                Layout.fillWidth: true
                text: backend ? backend.op_detail : ""
                color: theme ? theme.muted : "#9299a8"
                font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                elide: Text.ElideRight
            }
            Text {
                visible: root.backend && root.backend.batch_preview_error !== ""
                         && root.backend.batch_preview_rows.length === 0
                Layout.fillWidth: true
                text: root.backend ? root.backend.batch_preview_error : ""
                color: theme ? theme.danger : "#e06c75"
                font.pixelSize: 11 * (theme ? theme.font_scale : 1)
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                Text {
                    Layout.fillWidth: true
                    text: root.completed ? "Review each result above. Failed items were not overwritten."
                                         : "Confirming rechecks sources and destinations before any rename."
                    color: theme ? theme.muted : "#9299a8"
                    font.pixelSize: 10 * (theme ? theme.font_scale : 1)
                    wrapMode: Text.WordWrap
                }
                Rectangle {
                    width: cancelLabel.implicitWidth + 28
                    height: 34
                    radius: 6
                    color: cancelMouse.containsMouse ? (theme ? theme.hover : "#202531")
                                                     : (theme ? theme.surface_alt : "#1c202a")
                    border.width: 1
                    border.color: theme ? theme.border : "#292e3a"
                    Text {
                        id: cancelLabel
                        anchors.centerIn: parent
                        text: root.executing ? "Cancel safely" : root.completed ? "Done" : "Cancel"
                        color: theme ? theme.foreground : "#e7e9ee"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    }
                    MouseArea {
                        id: cancelMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            if (root.executing)
                                backend.cancel_operation()
                            else
                                root.close()
                        }
                    }
                }
                Rectangle {
                    visible: !root.completed
                    width: renameLabel.implicitWidth + 28
                    height: 34
                    radius: 6
                    opacity: root.canExecute ? 1 : 0.45
                    color: root.canExecute ? (theme ? theme.selection : "#24364f")
                                           : (theme ? theme.surface_alt : "#1c202a")
                    border.width: 1
                    border.color: theme ? theme.border : "#292e3a"
                    Text {
                        id: renameLabel
                        anchors.centerIn: parent
                        text: root.executing ? "Renaming\u2026" : "Rename " + root.affectedCount + " items"
                        color: theme ? theme.selection_text : "#f2f5fb"
                        font.pixelSize: 12 * (theme ? theme.font_scale : 1)
                    }
                    MouseArea {
                        anchors.fill: parent
                        enabled: root.canExecute
                        onClicked: root.execute()
                    }
                }
            }
        }
    }
}
