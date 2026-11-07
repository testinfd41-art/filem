local M = {}

local MAX_STATUS_AGE_MS = 5000

local function statusDirectory()
  local cacheHome = noctalia.getenv("XDG_CACHE_HOME")
  if cacheHome == nil or cacheHome == "" or cacheHome:sub(1, 1) ~= "/" then
    local home = noctalia.getenv("HOME") or ""
    cacheHome = home .. "/.cache"
  end
  return cacheHome .. "/filem/noctalia"
end

local function isFresh(snapshot, now)
  local updatedAt = tonumber(snapshot.updatedAtMs)
  return updatedAt ~= nil
    and updatedAt <= now + MAX_STATUS_AGE_MS
    and now - updatedAt <= MAX_STATUS_AGE_MS
end

function M.readJobs()
  local directory = statusDirectory()
  local files = noctalia.listDir(directory)
  if type(files) ~= "table" then
    return {}
  end

  local now = noctalia.nowMs()
  local jobs = {}
  for _, name in ipairs(files) do
    if type(name) == "string" and name:match("^transfers%-%d+%.json$") then
      local contents = noctalia.readFile(directory .. "/" .. name)
      if contents ~= nil then
        local ok, snapshot = pcall(noctalia.json.decode, contents)
        if ok
          and type(snapshot) == "table"
          and snapshot.schema == 1
          and type(snapshot.jobs) == "table"
          and isFresh(snapshot, now)
        then
          for _, job in ipairs(snapshot.jobs) do
            if type(job) == "table" then
              table.insert(jobs, job)
            end
          end
        end
      end
    end
  end

  local statusOrder = {
    running = 1,
    queued = 2,
    failed = 3,
    cancelled = 4,
    completed = 5,
  }
  table.sort(jobs, function(left, right)
    local leftOrder = statusOrder[left.status] or 6
    local rightOrder = statusOrder[right.status] or 6
    if leftOrder ~= rightOrder then
      return leftOrder < rightOrder
    end
    return (tonumber(left.startedAtMs) or 0) > (tonumber(right.startedAtMs) or 0)
  end)

  return jobs
end

function M.activeJobs(jobs)
  local active = {}
  for _, job in ipairs(jobs) do
    if job.status == "running" or job.status == "queued" then
      table.insert(active, job)
    end
  end
  return active
end

function M.basename(path)
  if type(path) ~= "string" or path == "" then
    return ""
  end
  local trimmed = path:gsub("/+$", "")
  return trimmed:match("([^/]+)$") or trimmed
end

function M.formatBytes(bytes)
  local value = tonumber(bytes) or 0
  if value >= 1024 * 1024 * 1024 then
    return string.format("%.1f GB", value / (1024 * 1024 * 1024))
  elseif value >= 1024 * 1024 then
    return string.format("%.1f MB", value / (1024 * 1024))
  elseif value >= 1024 then
    return string.format("%d KB", math.floor(value / 1024))
  end
  return string.format("%d B", math.floor(value))
end

function M.formatProgress(job)
  return math.floor(math.clamp(tonumber(job.progress) or 0, 0, 1) * 100)
end

function M.formatSize(job)
  if job.sizeKnown ~= true then
    return "Calculating size"
  end
  return M.formatBytes(job.bytesCompleted)
    .. " / "
    .. M.formatBytes(job.totalBytes)
end

function M.formatSpeed(bytesPerSecond)
  local value = tonumber(bytesPerSecond) or 0
  if value >= 1024 * 1024 then
    return string.format("%.1f MB/s", value / (1024 * 1024))
  elseif value >= 1024 then
    return string.format("%d KB/s", math.floor(value / 1024))
  end
  return string.format("%d B/s", math.floor(value))
end

return M
