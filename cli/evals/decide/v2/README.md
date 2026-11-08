# Fresh decision eval, v2

This directory holds a fresh decision eval set built for the first Jev gate run. It is separate from the 28-case smoke set in `..` and the 23-case held-out set, and it shares no fixture or instruction text with either.

## Contents

- `cases.json`: 333 labeled cases across 16 fixture pages.
- `fixtures/`: one HTML page per archetype.
- `generate.mjs`: deterministic generator for both files.
- `manifest.json`: seed, counts, and generation notes.
- `RESULTS.md`: written after the single live run.

## Label source and freeze policy

The labels were written from each page's element list and from the instruction intent in `generate.mjs`. They were not derived from Jev output, an LLM, or any prior run. No decider was called while building or validating this set.

The case and fixture bytes were committed before the live run. The commit that adds this directory is the freeze commit. After the live run, labels, fixtures, instructions, candidate construction, prompts, and criteria stay unchanged. Failures are reported, not tuned away. There are no excluded cases: every generated case is part of the denominator.

Freeze commit: `862091621c05f8bee52fadb7b0b9e877b3d397fd`.

## Page archetypes

The pages cover authentication, commerce catalog, checkout, admin settings, documentation search, travel search, finance dashboard, media player, data table pagination, modal dialog, long article, support ticket, profile editing, analytics, billing, and an onboarding wizard. Each page varies element order, duplicate distractor names, disabled controls, below-fold targets, and forms with supplied or missing values.

## Operation counts

| operation | cases |
| --- | --- |
| click | 109 |
| fill | 88 |
| select | 64 |
| scroll_down | 18 |
| scroll_up | 18 |
| back | 18 |
| wait | 18 |
| total | 333 |

Of the fill and select cases, 32 are blocked cases: the instruction names a real target, but the caller supplies no value, so the correct decision is the matching operation with a blocked reason.

## Regenerate

```bash
node cli/evals/decide/v2/generate.mjs
```

The generator is deterministic and records seed `20261108`. Fixture text and labels contain no randomness, so regenerating produces byte-identical files.

## Validation without a decider

The validation test launches headless Chrome, loads every fixture, snapshots it, builds the same candidate list `act` uses, and checks that each expected target exists exactly once and supports the expected operation. It also checks blocked cases supply no value and wait cases offer wait. It never constructs a decider.

```bash
cd cli
AGENT_BROWSER_DECIDE_CASES=evals/decide/v2/cases.json \
  cargo test e2e_decide_eval_validate -- --ignored --nocapture --test-threads=1
```

## Single live run

```bash
cd cli
doppler run -p dotfiles -c dev -- sh -c 'AGENT_BROWSER_DECIDER=jev AGENT_BROWSER_DECIDE_CASES=evals/decide/v2/cases.json cargo test e2e_decide_eval -- --ignored --nocapture --test-threads=1'
```

This command runs once. If infrastructure fails before any decision is made, the run may be repeated and the reason recorded. If any decision was made, the run stands and every failure is reported.

## Scoring

A case is correct when the operation matches, the target accessible name matches, and, for a blocked case, the decision carries a blocked reason. A wrong action is any incorrect decision or any decider error. The report includes total, correct, wrong, errors, blocked correctness, accuracy, wrong rate, a 95% Clopper-Pearson upper bound on the wrong rate, p50 and p95 decide latency, max latency, tokens per call, per-operation results, and every failure with expected and actual values.
