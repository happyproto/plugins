-- A handler written against the generated declarations. `--check` reports
-- any diagnostic as a failure, so a wrong type annotation in the generated
-- files, or a field this file reads that they do not declare, fails it.
local http = require("happyview.http")
local objects = require("happyview.objects")

---@param input lexicons.com.example.feed.getPosts.Params
---@param ctx happyview.QueryCtx
---@return lexicons.com.example.feed.getPosts.Output
function handle(input, ctx)
  local actor = input.actor
  local caller = ctx.caller_did
  local response = http.get("https://example.com", { headers = {} })
  local status = response.status
  local doc = objects.chain():add():add():doc()

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
