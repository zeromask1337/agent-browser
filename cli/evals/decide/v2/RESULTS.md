# Fresh decision eval v2 results

Run date: 2026-11-08
Model: `jev-latest` (TypeSafe default, no `TYPESAFE_MODEL` override)
Freeze commit: `862091621c05f8bee52fadb7b0b9e877b3d397fd`
Pre-run HEAD: `08ebe7c13604c2d30b3c4428bd8899a04c92fb5a`
Results commit: added after the run, docs only.

## Run provenance

- One live eval run. Exit code 0. 333 decisions were made.
- The command was exactly the single-run command documented in `README.md`, with `AGENT_BROWSER_DECIDER=jev` and `AGENT_BROWSER_DECIDE_CASES=evals/decide/v2/cases.json`.
- No rerun, retry of the suite, or partial rerun occurred.
- The cargo filter `e2e_decide_eval` also matched `e2e_decide_eval_validate`. The validation test ran after the scored test and made no decider call.
- The cases, fixtures, instructions, labels, candidate construction, prompt, and criteria were not changed after seeing these results.
- No case was excluded. The denominator is all 333 generated cases.

## Headline metrics

| metric | value |
| --- | --- |
| total | 333 |
| correct | 307 |
| wrong | 26 |
| errors | 12 |
| accuracy | 92.2% |
| wrong rate | 7.8% |
| blocked correct | 20/32 (62.5%) |
| wrong-rate 95% Clopper-Pearson upper bound | 10.7% |
| decide latency p50 | 211 ms |
| decide latency p95 | 345 ms |
| decide latency max | 622 ms |
| tokens per scored case | 3717 input, 286 output |

Errors are included in wrong actions. Latency is measured around `decide_with_retry`; a case that retried includes both attempts.

The Clopper-Pearson upper bound is one-sided 95% for 26 wrong outcomes in 333 cases. It is computed from the exact binomial distribution, not from a normal approximation. The point wrong rate is 7.8%, and the upper bound is 10.7%.

## Gate

| gate | result |
| --- | --- |
| point wrong rate under 1% | fail (7.8%) |
| wrong-rate 95% upper bound under 1% | fail (10.7%) |
| p50 under 1000 ms | pass (211 ms) |

The point estimate does not meet the 1% gate. The 95% upper bound is also above 1%, so this run provides no statistical evidence that the true wrong rate is under 1%. It does not claim otherwise.

## By operation

| operation | correct | total | accuracy |
| --- | --- | --- | --- |
| back | 10 | 18 | 55.6% |
| click | 106 | 109 | 97.2% |
| fill | 88 | 88 | 100.0% |
| scroll_down | 17 | 18 | 94.4% |
| scroll_up | 16 | 18 | 88.9% |
| select | 52 | 64 | 81.3% |
| wait | 18 | 18 | 100.0% |

All 16 blocked fill cases were correct. Only 4 of 16 blocked select cases were correct; the other 12 raised the invalid-target error listed below.

## Failures

Every failed case is listed. Expected target is the labeled accessible name. Expected status is `blocked` for a blocked case and `-` otherwise. Got is the returned operation and target, or the decider error. Latency is the full decide sequence in milliseconds.

| case | expected op | expected target | expected status | got | got target | ms |
| --- | --- | --- | --- | --- | --- | --- |
| `auth_portal-select-region-blocked` | select | Region | blocked | error: The chosen target does not support the operation. | - | 434 |
| `shop_catalog-click-in-stock-only` | click | In stock only | - | done | - | 177 |
| `checkout_flow-select-delivery-speed-blocked` | select | Delivery speed | blocked | error: The chosen target does not support the operation. | - | 622 |
| `checkout_flow-back` | back | - | - | click | Return to cart | 213 |
| `checkout_flow-back-cart` | back | - | - | click | Return to cart | 221 |
| `admin_settings-click-general` | click | General | - | done | - | 201 |
| `admin_settings-select-default-role-blocked` | select | Default role | blocked | error: The chosen target does not support the operation. | - | 391 |
| `docs_search-select-version-blocked` | select | Version | blocked | error: The chosen target does not support the operation. | - | 418 |
| `docs_search-back` | back | - | - | click | Previous article | 206 |
| `finance_dashboard-click-overview` | click | Overview | - | done | - | 218 |
| `finance_dashboard-select-account-blocked` | select | Account | blocked | error: The chosen target does not support the operation. | - | 511 |
| `media_player-select-quality-blocked` | select | Quality | blocked | error: The chosen target does not support the operation. | - | 418 |
| `media_player-scroll_up` | scroll_up | - | - | done | - | 283 |
| `data_table-select-rows-per-page-blocked` | select | Rows per page | blocked | error: The chosen target does not support the operation. | - | 371 |
| `modal_dialog-select-permission-blocked` | select | Permission | blocked | error: The chosen target does not support the operation. | - | 408 |
| `modal_dialog-back` | back | - | - | click | Close dialog | 311 |
| `long_article-select-text-size-blocked` | select | Text size | blocked | error: The chosen target does not support the operation. | - | 415 |
| `long_article-scroll_down` | scroll_down | - | - | done | - | 174 |
| `long_article-back` | back | - | - | click | Article list | 174 |
| `support_ticket-select-priority-blocked` | select | Priority | blocked | error: The chosen target does not support the operation. | - | 418 |
| `support_ticket-back` | back | - | - | click | Back to tickets | 187 |
| `profile_edit-select-language-blocked` | select | Language | blocked | error: The chosen target does not support the operation. | - | 478 |
| `billing_settings-select-plan-blocked` | select | Plan | blocked | error: The chosen target does not support the operation. | - | 401 |
| `billing_settings-scroll_up` | scroll_up | - | - | done | - | 196 |
| `billing_settings-back` | back | - | - | click | Back to account | 198 |
| `onboarding_wizard-back` | back | - | - | click | Back | 208 |

## Failure clusters

| cluster | failures |
| --- | --- |
| history back answered with a visible back, previous, or return link or button | 8 |
| tab or checkbox treated as already done | 3 |
| scroll instruction answered with done | 3 |
| blocked select answered with an invalid target, then errored after one retry | 12 |

These clusters describe this run only. They are not a basis for post-run label or prompt changes.

## Pre-run validation

Before the live run, `e2e_decide_eval_validate` loaded all 333 fixtures in headless Chrome, checked that every expected target appears exactly once among offered candidates, that the target role supports the expected operation, that blocked cases supply no value, that value cases supply a value, and that wait cases offer wait. The validation test does not construct a decider. It passed 333/333.

## Verify

```bash
cd cli
AGENT_BROWSER_DECIDE_CASES=evals/decide/v2/cases.json \
  cargo test e2e_decide_eval_validate -- --ignored --nocapture --test-threads=1
```

To compare a future run, use the single live command in `README.md` against the frozen cases. Any label or fixture change invalidates this result.
