-- A handler written against the generated declarations. `--check` reports
-- any diagnostic as a failure, so a wrong type annotation in the generated
-- files, or a field this file reads that they do not declare, fails it.
local http = require("happyview.http")
local db = require("happyview.db")
local record = require("happyview.record")

---@param input lexicons.com.example.feed.getPosts.Params
---@param ctx happyview.QueryCtx
---@return lexicons.com.example.feed.getPosts.Output
function handle(input, ctx)
  local actor = input.actor
  local caller = ctx.caller_did
  local response = http.get("https://example.com", { headers = {} })
  local status = response.status
  local page = db.records("com.example.feed.post"):where("text", "like", "%hello%"):limit(input.limit):run()
  local first = page.records[1] and page.records[1].uri
  local written = record.create("com.example.feed.post", { text = "hi" }, { rkey = "1" })
  local doc = tostring(first) .. written.uri .. tostring(page.cursor)

  ---@type lexicons.com.example.feed.post.Record
  local post = {
    text = actor .. tostring(caller) .. tostring(status) .. tostring(doc),
    createdAt = "2026-10-24T00:00:00Z",
  }
  return { posts = { post } }
end

---@param ctx happyview.JobCtx
function job(_, ctx)
  ctx.job.progress({ done = 1 })
  if ctx.job.should_stop() then
    return
  end
  ctx.job.wait(5)
end

---@param event lexicons.com.example.feed.post.Event
---@return string?
function on_post(event)
  return event.record and event.record.text
end
