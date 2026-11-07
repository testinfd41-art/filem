local status = require("./lib/status.luau")

local function jobCard(job)
  local title = job.operation .. " · " .. job.status
  local source = status.basename(job.currentItem ~= "" and job.currentItem or job.source)
  local destination = status.basename(job.destination)
  local route = source ~= "" and destination ~= ""
    and (source .. " → " .. destination)
    or (source ~= "" and source or (destination ~= "" and destination or job.summary))

  local details = {
    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = title, fontWeight = "bold", flexGrow = 1 }),
      ui.label({ text = status.formatProgress(job) .. "%", color = "primary" }),
    }),
    ui.label({ text = route, color = "on_surface_variant", maxLines = 1 }),
  }

  if job.status == "running" or job.status == "queued" then
    table.insert(details, ui.progress({
      progress = status.formatProgress(job) / 100,
      fill = "primary",
      track = "surface",
      height = 6,
    }))
    local fileCount = tostring(job.completedFiles or 0)
      .. " / "
      .. tostring(job.totalFiles or 0)
      .. " files"
    local size = status.formatSize(job)
    table.insert(details, ui.row({ gap = 10, align = "center" }, {
      ui.label({ text = fileCount, color = "on_surface_variant", flexGrow = 1 }),
      ui.label({ text = size, color = "on_surface_variant" }),
    }))
    if tonumber(job.bytesPerSecond) and job.bytesPerSecond > 0 then
      table.insert(details, ui.label({
        text = status.formatSpeed(job.bytesPerSecond),
        color = "on_surface_variant",
      }))
    end
    if job.currentItem ~= "" then
      table.insert(details, ui.label({
        text = job.currentItem,
        color = "on_surface_variant",
        maxLines = 1,
      }))
    end
  elseif job.error ~= "" then
    table.insert(details, ui.label({ text = job.error, color = "error", maxLines = 2 }))
  elseif job.summary ~= "" then
    table.insert(details, ui.label({ text = job.summary, color = "on_surface_variant", maxLines = 2 }))
  end

  return ui.column({
    key = job.id,
    gap = 6,
    padding = 10,
    fill = "surface",
    radius = 8,
  }, details)
end

local function render()
  local jobs = status.readJobs()
  local content = {
    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = "Filem transfers", fontSize = 16, fontWeight = "bold", flexGrow = 1 }),
      ui.button({ text = "Open Filem", onClick = "openFilem" }),
      ui.button({ glyph = "close", tooltip = "Close", onClick = "closePanel" }),
    }),
  }

  if #jobs == 0 then
    table.insert(content, ui.label({
      text = "No active Filem transfers.",
      color = "on_surface_variant",
      flexGrow = 1,
    }))
  else
    local cards = {}
    for index = 1, math.min(#jobs, 20) do
      table.insert(cards, jobCard(jobs[index]))
    end
    table.insert(content, ui.scroll({ flexGrow = 1, gap = 10 }, cards))
  end

  panel.render(ui.column({ flexGrow = 1, gap = 12 }, content))
end

function onOpen()
  render()
end

function update()
  noctalia.setUpdateInterval(1000)
  render()
end

function closePanel()
  panel.close()
end

function openFilem()
  local started = noctalia.runAsync({ "filem" })
  if not started then
    noctalia.notifyError("Filem", "Could not start Filem.")
  end
end
