local status = require("./lib/status.luau")

local function render()
  local jobs = status.activeJobs(status.readJobs())
  local job = jobs[1]
  barWidget.setVisible(true)
  if job == nil then
    barWidget.render(ui.row({ gap = 5, align = "center" }, {
      ui.glyph({ name = "sync_alt", size = 14, color = "primary" }),
      ui.label({ text = "Filem", fontSize = 12, fontWeight = "medium" }),
    }))
    barWidget.setTooltip({ { key = "Status", value = "No active transfers" } })
    return
  end

  local progress = status.formatProgress(job)
  local label = job.status == "queued"
    and (job.operation .. " queued")
    or (job.operation .. " " .. progress .. "%")
  if #jobs > 1 then
    label = #jobs .. " transfers"
  end

  barWidget.render(ui.row({ gap = 5, align = "center" }, {
    ui.glyph({ name = "sync_alt", size = 14, color = "primary" }),
    ui.label({ text = label, fontSize = 12, fontWeight = "medium" }),
  }))

  local tooltip = {
    { key = job.operation, value = job.status == "running" and (progress .. "%") or "Queued" },
    { key = "From", value = status.basename(job.currentItem ~= "" and job.currentItem or job.source) },
    { key = "To", value = status.basename(job.destination) },
  }
  if job.sizeKnown == true then
    table.insert(tooltip, { key = "Size", value = status.formatSize(job) })
  end
  if tonumber(job.bytesPerSecond) and job.bytesPerSecond > 0 then
    table.insert(tooltip, { key = "Speed", value = status.formatSpeed(job.bytesPerSecond) })
  end
  barWidget.setTooltip(tooltip)
end

function update()
  noctalia.setUpdateInterval(1000)
  render()
end

function onClick()
  noctalia.togglePanel("testinfd41-art/filem-transfers:manager")
end
