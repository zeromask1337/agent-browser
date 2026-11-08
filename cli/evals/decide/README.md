# Decision eval

Hand-labeled cases for the `act` decider. Each case names a fixture page, one instruction, optional caller-supplied values, and the expected operation and target accessible name. Labels come from reading the fixtures, not from model output.

Run the ignored Rust test against a live decider:

```bash
cd cli
TYPESAFE_API_KEY=... AGENT_BROWSER_DECIDER=jev \
  cargo test --bin agent-browser native::e2e_tests::e2e_decide_eval -- \
  --ignored --exact --nocapture --test-threads=1
```

The runner reads `cases.json` by default. Point it at another file with `AGENT_BROWSER_DECIDE_CASES`:

```bash
cd cli
TYPESAFE_API_KEY=... AGENT_BROWSER_DECIDER=jev \
  AGENT_BROWSER_DECIDE_CASES=evals/decide/cases-heldout.json \
  cargo test --bin agent-browser native::e2e_tests::e2e_decide_eval -- \
  --ignored --exact --nocapture --test-threads=1
```

Fixtures resolve from `fixtures/` beside the cases file first, then from `src/native/test_fixtures/`, so a cases file can share the native test fixture pages.

`cases-heldout.json` is a separate held-out set. It was written and labeled before any live run and was not used to tune prompts, criteria, or labels.

`v4/` is the frozen fresh set used for the latest run. Its generator, fixtures, cases, and labels were committed before the run, and `v4/RESULTS.md` records the single scored run. `v3/` remains the previous fresh set and result.

The runner launches one headless Chrome, loads each fixture as a data URL, snapshots it, builds the same candidate list `act` uses, and asks the decider once per case. It does not click, fill, or navigate. It prints a per-case table and these numbers:

- Decision correct and wrong: operation, target, and blocked status all match the label. Decider errors count as wrong.
- Executed total: decisions the auto-act gate would run, meaning not blocked, stale, dry run, below the confidence floor, or confirmation required.
- Executed wrong: a would-execute decision whose operation or target differs from the label. The name means "would execute"; the runner does not claim the browser ran it.
- Executed wrong over total and over executed total, coverage, and blocked outcomes separately.
- A one-sided 95% Clopper-Pearson upper bound for executed-wrong under both denominators.
- p50 and p95 decide latency, measured around the decider call only.
- Tokens per call, when the provider reports usage.

## Gates

These are the gates to report against. The repository does not tune labels or thresholds to pass them, and a failure is reported as a failure.

- Executed wrong under 1% of all cases.
- Executed wrong under 1% of executed decisions.
- Coverage at least 90%, so a run cannot pass by blocking everything.
- p50 per decision under one second from the machine that ran the test.

Decision wrong is reported honestly alongside these gates. A blocked mismatch is a utility failure, not an executed-wrong failure.

A 95% Clopper-Pearson upper bound on both executed-wrong rates is reported with every fresh run. The point estimates and the upper bounds are both evidence; neither is treated as proof when a bound is above the gate.

The LLM comparison arm is removed. It is not part of the gate, and a fresh run is not required to call an LLM decider.

## Known gap

The frozen sets are synthetic fixtures built for this repository. They do not replace labeled real snapshots collected from actual sites. Real pages add duplicate names, long candidate lists, iframes, shadow DOM, sticky headers, and layout noise that these fixtures do not reproduce. Treat the numbers as a direction, not a benchmark. The v4 set has no destructive-labeled cases, so it does not cover confirmation behavior.

## Adding a case

Add a fixture under `fixtures/`, then add an entry to `cases.json`:

```json
{
  "name": "login-click-sign-in",
  "fixture": "login.html",
  "instruction": "Click the Sign in button",
  "values": { "email": "hiker@example.com" },
  "expect": { "operation": "click", "target_name": "Sign in" }
}
```

`target_name` is the accessible name shown in a snapshot. Omit it for operations that do not take a target: `scroll_down`, `scroll_up`, `back`, and `wait`.

Set `expect.blocked` to `true` when the correct operation needs a value the caller did not supply. The decision then passes only when it is blocked, not when it is an error.
