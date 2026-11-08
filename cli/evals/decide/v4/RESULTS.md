# Fresh decision eval v4 results

Run date: 2026-11-08
Model: `jev-latest` (TypeSafe default, no `TYPESAFE_MODEL` override)
Provider model reported by the API: `jev-1.14.0`
Freeze commit: `51fd45b915f513241536a3974fd6a61cfb03b910`
Pre-run HEAD: `51fd45b915f513241536a3974fd6a61cfb03b910`
Results commit: added after the run, docs only.

## Run provenance

- One scored live run. Exit code 0. 440 decisions were made.
- The command was the single-run command in `README.md`, with
  `AGENT_BROWSER_DECIDER=jev` and
  `AGENT_BROWSER_DECIDE_CASES=evals/decide/v4/cases.json`.
- No rerun of the scored suite, retry of the suite, or partial rerun occurred.
- The exact test filter was
  `native::e2e_tests::e2e_decide_eval -- --ignored --exact`, so the
  validation test did not run in the same invocation. It had already passed
  440/440 before the freeze.
- No case, fixture, instruction, label, candidate construction, prompt, or
  criterion was changed after seeing these results. Only this document was
  added after the run.
- No case was excluded. The denominator is all 440 frozen cases.

### Aborted environment attempt

At 16:23:40Z the same frozen command was attempted once before the scored run.
Every request received HTTP 451 from the provider:

```text
{"title":"Typesafe is not available in your region.","status":451}
```

The runner maps non-success status 451 to a malformed-request error, so that
invocation produced 440 errors and zero decisions. It is not scored evidence.
The machine then connected its saved Latvia VPN, a direct request to
`POST /v1/systemone` returned HTTP 200, and the scored run started at
16:32:01Z. No repository file, dataset, label, or decider setting changed
between the freeze commit and the scored run. The aborted log is outside the
repository and is not part of the result.

## Headline metrics

| metric | value |
| --- | --- |
| total | 440 |
| decision correct | 440 (100.0%) |
| decision wrong | 0 (0.0%) |
| errors | 0 |
| executed total (would execute) | 414 |
| executed wrong (would execute wrong) | 0 |
| executed wrong / total | 0.00% |
| executed wrong / executed total | 0.00% |
| coverage (executed / total) | 94.1% (414/440) |
| blocked outcomes | 26 (5.9%) |
| labeled blocked correct | 26/26 (100.0%) |
| executed-wrong 95% Clopper-Pearson upper, of total | 0.68% |
| executed-wrong 95% Clopper-Pearson upper, of executed | 0.72% |
| decide latency p50 | 218 ms |
| decide latency p95 | 303 ms |
| decide latency max | 562 ms |
| tokens per call | 5096 input, 293 output |

The runner only decides and classifies; it does not click, fill, or navigate.
`executed wrong` therefore means **would execute wrong** under the same gate
`handle_act_with` uses. `executed_total` counts decisions with no blocked
reason, no stale page, no dry run, no confidence-floor block, and no
confirmation requirement. The confidence floor is the `act` default of none.

The Clopper-Pearson bounds are one-sided 95%, computed from the exact binomial
distribution. For 0 wrong in 440, the bound is 0.68%; for 0 wrong in 414
executed, the bound is 0.72%. Both are below 1%.

## Gate

| gate | result |
| --- | --- |
| executed wrong / total under 1% | pass (0.00%) |
| executed wrong / executed total under 1% | pass (0.00%) |
| coverage at least 90% | pass (94.1%) |
| decision wrong under 1% | pass (0.00%) |
| p50 under 1000 ms | pass (218 ms) |

The point estimates pass every gate, and both exact upper bounds are under 1%.
No decision was wrong, so there is no failure to list.

## By operation

| operation | correct | total | executed | executed wrong |
| --- | --- | --- | --- | --- |
| back | 22 | 22 | 22 | 0 |
| click | 154 | 154 | 154 | 0 |
| fill | 110 | 110 | 97 | 0 |
| scroll_down | 22 | 22 | 22 | 0 |
| scroll_up | 22 | 22 | 22 | 0 |
| select | 88 | 88 | 75 | 0 |
| wait | 22 | 22 | 22 | 0 |

All 26 blocked fill and select cases returned status blocked with a reason.
Thirteen blocked fills and thirteen blocked selects account for the 26 cases
that did not count as executed.

## Failures

None. The run printed zero `MISS` lines and zero `EXECUTED-WRONG` lines.

## Destructive-label limitation

The frozen set has zero destructive labels. These fixtures have no destructive
actions. The runner supports a `destructive` case label: a labeled case is
classified `confirmation_required` and never counts as executed. That path was
not exercised in this run, and `act` confirmation is a caller-configured policy
(`--confirm-actions`), not automatic page-content detection. This result does
not claim that destructive browser actions were tested.

## Pre-run validation

Before the freeze, `e2e_decide_eval_validate` loaded all 440 v4 fixtures in
headless Chrome and checked that every expected target appears exactly once
among offered candidates, that the target role supports the expected operation,
that blocked cases supply no value, that value cases supply a value, that wait
cases offer wait, and that wait is not offered when the instruction names the
expected target. It passed 440/440 across 22 pages, with 26 blocked cases and
0 destructive labels.

## Verify

```bash
cd cli
AGENT_BROWSER_DECIDE_CASES=evals/decide/v4/cases.json \
  cargo test --bin agent-browser native::e2e_tests::e2e_decide_eval_validate -- \
  --ignored --exact --nocapture --test-threads=1
```

To compare a future run, use the single live command in `README.md` against the
frozen cases at `51fd45b915f513241536a3974fd6a61cfb03b910`. Any label or fixture
change invalidates this result.
