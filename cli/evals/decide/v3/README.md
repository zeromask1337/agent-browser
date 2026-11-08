# Fresh decision eval v3

Frozen held-out set for the `act` decider. It was generated and labeled from
fixture construction before any decider call. No case is derived from v2 model
output, and no case is excluded after a run.

- 16 fixture pages, 333 cases.
- Operations: click 93, fill 108, select 64, scroll_down 16, scroll_up 16, back 18, wait 18.
- 32 blocked cases: fill or select requests with no caller-supplied value.
- No case expects `done`; `act` no longer offers it.

Regenerate both fixtures and cases with:

```bash
node cli/evals/decide/v3/generate.mjs
```

Validate every fixture and label without constructing a decider:

```bash
cd cli
AGENT_BROWSER_DECIDE_CASES=evals/decide/v3/cases.json \
  cargo test native::e2e_tests::e2e_decide_eval_validate -- \
  --ignored --exact --nocapture --test-threads=1
```

Run the scored set exactly once:

```bash
cd cli
AGENT_BROWSER_DECIDER=jev TYPESAFE_API_KEY=... \
AGENT_BROWSER_DECIDE_CASES=evals/decide/v3/cases.json \
  cargo test native::e2e_tests::e2e_decide_eval -- \
  --ignored --exact --nocapture --test-threads=1
```

`RESULTS.md` records the single run. Do not change a label, fixture, instruction,
candidate construction, or prompt after seeing a result.
