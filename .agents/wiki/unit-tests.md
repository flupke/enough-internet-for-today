# Tests

Which tests to write and keep, and the tautological tests code review removes.

## Policy

Test the behaviour that matters once, at the lowest level that sees it: a unit test for the
collector's logic, `tests/collect.rs` for a collection end to end with the stand-in `hc`, and
`tests/panel` for the one flow the user sees, the panel showing what the collector wrote. No
tests for crash, restart or race recovery, or for the helper's prompt wording, unless the user
asks. Every change deletes the tests it makes obsolete or redundant, and a smaller suite counts
as progress.

A test earns its place when it checks behaviour a user or caller relies on, that the code does
not make obvious, at the lowest level that sees it, and that no other test checks. Judge it by
two edits: break the behaviour (a wrong value, an effect skipped or done twice) and it fails;
change only the internals, with every observable result the same, and it still passes. A test
that survives the first edit is tautological; one that fails the second is pinned to the
implementation: rewrite it to assert what users observe, or delete it.

## Tests that can go red

- The expected value is written by hand from the behaviour the code owes, or comes from an
  independent source: an external tool's output, a second code path.
- The assertion reads what production code produced: a return value, a file the collector
  wrote, the panel's `digest.debug`, a screenshot.
- A format this repository owns, such as the zone files, the config or the job files, is
  compared with a hand-written JSON literal, or with the fixtures of `tests/fixtures/`. A rename
  then breaks the test as it would break the files users already have.
- No test asserts the helper's prompt wording. Test what the code computes for a job, such as
  its candidates and limits, only when computing it is not trivial.

## Tautological tests

Code review deletes a tautological test, or rewrites it to assert what the code computes. The
kinds:

- **Mirror**: the expected value is computed by the code under test, or by a copy of its logic,
  so a bug lands on both sides.
- **Echo**: the test reads back a field it set, or a clone.
- **Constant pin**: `assert_eq!(RESULT_LIMIT, 4 * 1024 * 1024)`. Build the boundary input from
  the constant instead.
- **Library behaviour**: std, serde_json, jiff, url, a dependency's own behaviour. The
  dependency's own tests cover them.
- **Fake echo**: every value asserted comes from the stand-in `hc` or a fake `World` the test
  configured, through code that only passes it on.
- **Vacuous assertion**: true for every value the type can hold, or met by an error as well as a
  success.

## Waiting

A test never waits on the wall clock for the collector: give it its clock as a value the test
sets (the `World` trait's `now` and `local_now`), and wake the test on what the collector
reports, such as its exit or a file it wrote.

The panel sends no event: `tests/panel` waits for `digest.debug` in the app's `shared.json` to
record the act it awaits, and for the window to show it. Those are the only polls a test may
run. A test may block without a deadline of its own: nextest ends one still running after two
minutes (`.config/nextest.toml`) and reports it as failed. A local deadline is worth it only to
say what was awaited.
