# Decision eval

Hand-labeled cases for the `act` decider. Each case names a fixture page, one instruction, optional caller-supplied values, and the expected operation and target accessible name. Labels come from reading the fixtures, not from model output.

Run the ignored Rust test against a live decider:

```bash
cd cli
TYPESAFE_API_KEY=... AGENT_BROWSER_DECIDER=jev \
  cargo test e2e_decide_eval -- --ignored --nocapture
```

For the LLM baseline on the same cases:

```bash
cd cli
AI_GATEWAY_API_KEY=... AGENT_BROWSER_DECIDER=llm \
  cargo test e2e_decide_eval -- --ignored --nocapture
```

The runner reads `cases.json` by default. Point it at another file with `AGENT_BROWSER_DECIDE_CASES`:

```bash
cd cli
TYPESAFE_API_KEY=... AGENT_BROWSER_DECIDER=jev \
  AGENT_BROWSER_DECIDE_CASES=evals/decide/cases-heldout.json \
  cargo test e2e_decide_eval -- --ignored --nocapture --test-threads=1
```

Fixtures resolve from `fixtures/` beside the cases file first, then from `src/native/test_fixtures/`, so a cases file can share the native test fixture pages.

`cases-heldout.json` is a separate held-out set. It was written and labeled before any live run and was not used to tune prompts, criteria, or labels.

The runner launches one headless Chrome, loads each fixture as a data URL, snapshots it, builds the same candidate list `act` uses, and asks the decider once per case. It prints a per-case table and these numbers:

- Top-1 accuracy: operation and target both correct.
- Wrong actions: any case where the operation was wrong, the target name was wrong, or the decider returned an invalid answer.
- p50 and p95 decide latency, measured around the decider call only.
- Tokens per call, when the provider reports usage.

## Gates

These are the gates to report against. The repository does not tune labels or thresholds to pass them, and a failure is reported as a failure.

- Wrong actions under 1% at the chosen confidence threshold.
- p50 per decision under one second from the machine that ran the test.
- The LLM arm runs on the same cases, so the two reports are comparable.

## Known gap

These 28 cases are a smoke set built for this repository. They do not replace the 50 to 100 labeled real snapshots that still need collecting from actual sites. Real pages add duplicate names, long candidate lists, iframes, shadow DOM, sticky headers, and layout noise that these fixtures do not reproduce. Treat the numbers below as a direction, not a benchmark.

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

`target_name` is the accessible name shown in a snapshot. Omit it for operations that do not take a target: `scroll_down`, `scroll_up`, `back`, `wait`, and `done`.

Set `expect.blocked` to `true` when the correct operation needs a value the caller did not supply. The decision then passes only when it is blocked, not when it is an error.
