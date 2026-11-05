# Enough Internet for Today

A finite digest, read in a herdr-app panel, of the Slack channels the user belongs to, their
Gmail inbox, and what they follow on social media: a few items per zone, then done for now.

## Language

**Zone**:
One of the panel's three parts, Slack, Email and Social, each with its sources, its schedule,
its own collection, last collection time and failure. `data/digest/<zone>.json` in herdr-app's
home holds its state and its selection. Slack keeps its selections of the last day; Email and
Social keep their latest.
_Avoid_: section, panel (the panel is the mod)

**Digest**:
What the panel shows: every zone's selection, at most five items per collection.
_Avoid_: feed, newsletter, report

**Selection**:
The items one collection chose, at most five; empty when nothing was worth it.

**Item**:
One entry of a zone's selection: a title, a summary, why it matters, its source, its original
link, and when it was selected.
_Avoid_: card, post (a post is X's)

**Source**:
Where candidates come from: X (Social's, for now), Slack or Email.
_Avoid_: channel (a channel is Slack's), provider

**Candidate**:
A post, message or thread a source gave for the helper to select from.

**Collection**:
One zone's run of `herdr-digest collect`: gather the candidates newer than its cursor, have the
helper select the items, update the zone. A failed collection leaves the zone's selection and
cursor in place; one that finds nothing new keeps the selection and selects nothing.
_Avoid_: sync, fetch

**Refresh**:
The panel's button on each zone, which runs that zone's collection at once; "refreshed" is when
a zone's last collection succeeded.

**Slot**:
A time a zone is due: every morning (`morning`, 08:00 by default) for Email and Social, every
`slack_every_minutes` (30 by default) for Slack. The timer collects each zone whose current slot
has no collection yet, and retries a failed one at most every 30 minutes.

**Cursor**:
Per source, where its next collection starts, strictly after it: the newest X post ID seen, and
for Slack and Email the time up to which they were collected. It moves only once the
collection's selection is safely kept.
_Avoid_: watermark

**Pending batch**:
The X posts a collection fetched but whose selection failed, kept so a retry does not pay X
again.

**Helper**:
The persistent Claude Sonnet session, `enough-internet`, that `hc` runs: it gathers Slack and
email through the user's MCP servers and selects the items. It is read-only towards them.
_Avoid_: agent, bot, LLM command

**Job**:
One request to the helper, for one zone: an input file and a result file under the helper's
`jobs` folder.

**Panel**:
The herdr-app `digest` mod, which shows the zones' unread items and ends on "Done for now",
which marks them read.
_Avoid_: widget, UI

**Demo mode**:
The panel opened with `{"demo": true}`, where the `digest.sample`, `digest.empty` and
`digest.failure` commands replace what it shows.
