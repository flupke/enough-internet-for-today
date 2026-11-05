# A file and a schedule per zone

The panel has three zones, Slack, Email and Social, which the user wants refreshed at different
rates: Slack every half hour, Email and Social once every morning, each with a manual refresh.
Before them, one collection gathered X, Slack and email together at the daily `times` slots and
wrote one digest file, `latest.json`, with every source's cursor in it.

Each zone now has its own collection, slot and file, `data/digest/<zone>.json`, which holds its
selection, cursor, last refresh and failure. A collection reads and writes its zone's file alone,
under the one worker lock, and the helper gets one job per zone. A zone that fails keeps its
selection and cursor without touching the others. The single digest file is migrated into the
zone files once (`src/legacy.rs`) and removed.

Given up: one file holding every zone, rewritten by each collection. Every half-hourly Slack run
would rewrite the morning zones' state, so a bug or a crash there could move a cursor it had no
business touching, and failures and refresh times per zone would live in a shared record that
each run must merge.
