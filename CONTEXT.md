# Enough Internet for Today

A finite digest, read in a herdr-app panel, of what the user follows on X, the Slack channels
they belong to, and their Gmail inbox: a few items, then done for now.

## Language

**Digest**:
One collection's result, at most five items across every source, with each source's coverage;
`data/digest/latest.json` in herdr-app's home holds the last one.
_Avoid_: feed, newsletter, report

**Item**:
One entry of a digest: a title, a summary, why it matters, its source and its original link.
_Avoid_: card, post (a post is X's)

**Source**:
Where candidates come from: X, Slack or Email. Each has its own coverage line, failure and
watermark.
_Avoid_: channel (a channel is Slack's), provider

**Candidate**:
A post, message or thread a source gave for the helper to select from.

**Collection**:
One run of `herdr-digest collect`: gather the candidates, have the helper select the items,
write the digest. A failed collection leaves the previous digest in place.
_Avoid_: sync, fetch, refresh

**Slot**:
One of the daily times (`times` in the config, 09:00 and 18:00 by default). The timer runs a
collection when the current slot has no digest yet.

**Watermark**:
Per Slack and Email, the time up to which they were collected; the next collection starts from
it. X's equivalent is its **cursor**, the newest post ID seen.

**Pending batch**:
The X posts a collection fetched but whose selection failed, kept so a retry does not pay X
again.

**Helper**:
The persistent Claude Sonnet session, `enough-internet`, that `hc` runs: it gathers Slack and
email through the user's MCP servers and selects the items. It is read-only towards them.
_Avoid_: agent, bot, LLM command

**Job**:
One request to the helper: an input file and a result file under the helper's `jobs` folder.

**Panel**:
The herdr-app `digest` mod, which shows the latest digest and ends on "Done for now".
_Avoid_: widget, UI

**Demo mode**:
The panel opened with `{"demo": true}`, where the `digest.sample`, `digest.empty` and
`digest.failure` commands replace what it shows.
