# Fresh decision eval v4

Frozen held-out set for the `act` decider. It was generated and labeled from
the page definitions in `generate.mjs` before any decider call. No case is
derived from v3 model output, and no case is excluded after a run.

- 22 fixture pages, 440 cases.
- Operations: click 154, fill 110, select 88, scroll_down 22, scroll_up 22,
  back 22, wait 22.
- 26 blocked cases (5.9%): fill or select requests with no caller-supplied
  value.
- No case is labeled destructive. The fixtures contain no destructive actions,
  so this set cannot measure confirmation behavior. The runner classifies a
  case labeled `destructive` as `confirmation_required`, which never counts as
  executed, but that path is unexercised here.
- No case expects `done`; `act` no longer offers it.

## Metric definitions

- `decision_wrong`: the chosen operation, target, or blocked status differs
  from the label. Decider errors also count.
- `executed_total`: decisions the auto-act gate would run with no confirmation
  step (not blocked, stale, dry run, below the confidence floor, or
  confirmation required).
- `executed_wrong` / `would_execute_wrong`: a decision the gate would run whose
  operation or target differs from the label. The runner does not touch the
  page, so this name means "would execute", not "the browser executed it".
- `coverage`: `executed_total / total`.
- `blocked`: outcomes reported blocked, including over-cautious blocks.
- `blocked accuracy`: correct blocked labels over expected blocked labels.

A blocked mismatch is a utility failure, not an executed-wrong failure. The
gate uses two denominators, `executed_wrong / total` and
`executed_wrong / executed_total`, both under 1%, with coverage at least 90%.

## Regenerate

```bash
node cli/evals/decide/v4/generate.mjs
```

## Validate without a decider

```bash
cd cli
AGENT_BROWSER_DECIDE_CASES=evals/decide/v4/cases.json \
  cargo test --bin agent-browser native::e2e_tests::e2e_decide_eval_validate -- \
  --ignored --exact --nocapture --test-threads=1
```

## Run the scored set exactly once

```bash
cd cli
doppler run -p dotfiles -c dev -- sh -c \
  'AGENT_BROWSER_DECIDER=jev AGENT_BROWSER_DECIDE_CASES=evals/decide/v4/cases.json \
   cargo test --bin agent-browser native::e2e_tests::e2e_decide_eval -- \
   --ignored --exact --nocapture --test-threads=1'
```

`RESULTS.md` records the single run. Do not change a label, fixture,
instruction, candidate construction, prompt, or criterion after seeing a result.
