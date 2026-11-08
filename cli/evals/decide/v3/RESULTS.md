# Fresh decision eval v3 results

Run date: 2026-11-08
Model: `jev-latest` (TypeSafe default, no `TYPESAFE_MODEL` override)
Freeze commit: `15f28326990507bec0f2dfea780fc85e3d2996a2`
Pre-run HEAD: `15f28326990507bec0f2dfea780fc85e3d2996a2`
Results commit: added after the run, docs only.

## Run provenance

- One live eval run. Exit code 0. 333 decisions were made.
- The command was the single-run command in `README.md`, with
  `AGENT_BROWSER_DECIDER=jev` and
  `AGENT_BROWSER_DECIDE_CASES=evals/decide/v3/cases.json`.
- No rerun, retry of the suite, or partial rerun occurred.
- The exact test filter was `native::e2e_tests::e2e_decide_eval -- --ignored --exact`,
  so the validation test did not run in the same invocation. It had already
  passed 333/333 before the freeze.
- No case, fixture, instruction, label, candidate construction, prompt, or
  criterion was changed after seeing these results. Only this document was
  added after the run.
- No case was excluded. The denominator is all 333 frozen cases.
- No LLM decider call was made. The LLM comparison arm is removed from the gate.

## Headline metrics

| metric | value |
| --- | --- |
| total | 333 |
| correct | 325 |
| wrong | 8 |
| errors | 0 |
| accuracy | 97.6% |
| wrong rate | 2.4% |
| blocked correct | 32/32 (100%) |
| wrong-rate 95% Clopper-Pearson upper bound | 4.3% |
| decide latency p50 | 211 ms |
| decide latency p95 | 284 ms |
| decide latency max | 712 ms |
| tokens per scored case | 3399 input, 226 output |

Errors are included in wrong actions; this run had none. Latency is measured
around `decide_with_retry`; a case that retried includes both attempts.

The Clopper-Pearson upper bound is one-sided 95% for 8 wrong outcomes in 333
cases. It is computed from the exact binomial distribution, not from a normal
approximation. The point wrong rate is 2.4%, and the upper bound is 4.3%.

## Gate

| gate | result |
| --- | --- |
| point wrong rate under 1% | fail (2.4%) |
| wrong-rate 95% upper bound under 1% | fail (4.3%) |
| p50 under 1000 ms | pass (211 ms) |

The point estimate does not meet the 1% gate. The 95% upper bound is also above
1%, so this run does not provide statistical evidence that the true wrong rate
is under 1%. It does not claim otherwise.

## By operation

| operation | correct | total | accuracy |
| --- | --- | --- | --- |
| back | 18 | 18 | 100.0% |
| click | 85 | 93 | 91.4% |
| fill | 108 | 108 | 100.0% |
| scroll_down | 16 | 16 | 100.0% |
| scroll_up | 16 | 16 | 100.0% |
| select | 64 | 64 | 100.0% |
| wait | 18 | 18 | 100.0% |

All 32 blocked fill and select cases were correct: each returned status blocked
with a reason, and none raised an error.

## Failures

Every failed case is listed. Expected target is the labeled accessible name.
Got is the returned operation and target, with the blocked reason when present.

| case | expected op | expected target | got | got target | reason |
| --- | --- | --- | --- | --- | --- |
| `recipe_box-click-find-recipes` | click | Find recipes | fill | Search recipes | needs a caller-supplied value |
| `iot_console-click-search-devices` | click | Search devices | fill | Device search | needs a caller-supplied value |
| `iot_console-click-device-logs` | click | Device logs | select | Device logs | target does not support select; needs a caller-supplied value |
| `legal_review-click-find-contracts` | click | Find contracts | fill | Search contracts | needs a caller-supplied value |
| `warehouse_ops-click-find-orders` | click | Find orders | fill | Search orders | needs a caller-supplied value |
| `warehouse_ops-click-assign-dock` | click | Assign dock | select | Dock | needs a caller-supplied value |
| `photo_library-click-find-photos` | click | Find photos | fill | Search photos | needs a caller-supplied value |
| `event_agenda-click-find-sessions` | click | Find sessions | fill | Search sessions | needs a caller-supplied value |

## Failure clusters

| cluster | failures |
| --- | --- |
| a "search <items>" instruction without a supplied value chose the search field and blocked instead of the search button | 6 |
| a click instruction chose a dropdown value instead of the named action control | 2 |

These clusters describe this run only. They are not a basis for post-run label or
prompt changes. The first cluster is the cost of keeping fill and select
selectable when a value is missing: a search instruction that does not name a
value can be read as typing into the field rather than pressing the search
button.

## Pre-run validation

Before the live run, `e2e_decide_eval_validate` loaded all 333 v3 fixtures in
headless Chrome and checked that every expected target appears exactly once
among offered candidates, that the target role supports the expected operation,
that blocked cases supply no value, that value cases supply a value, that wait
cases offer wait, and that wait is not offered when the instruction names the
expected target. The validation test does not construct a decider. It passed
333/333.

## Verify

```bash
cd cli
AGENT_BROWSER_DECIDE_CASES=evals/decide/v3/cases.json \
  cargo test native::e2e_tests::e2e_decide_eval_validate -- \
  --ignored --exact --nocapture --test-threads=1
```

To compare a future run, use the single live command in `README.md` against the
frozen cases at `15f28326990507bec0f2dfea780fc85e3d2996a2`. Any label or fixture
change invalidates this result.
