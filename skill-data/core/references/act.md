# act

`act` asks a decider to choose one operation and one rendered target from the current snapshot, then runs that action. Use it when the next step is clear in words but the ref is not, or when several similar elements make a deterministic locator ambiguous.

```bash
agent-browser snapshot -i
agent-browser act "<instruction>" [--dry-run] [--min-confidence <0..1>] [--value <key>=<value>]...
```

Run `snapshot` first. `act` offers refs from the current snapshot and does not create new ones. If it reports `stale` or no candidates, snapshot again.

## Operations

The decider picks one of `click`, `fill`, `select`, `scroll_down`, `scroll_up`, `back`, `wait`, and `done`. `click`, `fill`, and `select` need a target. `scroll_down` and `scroll_up` move 600 px. `wait` pauses for 500 ms. `done` means the instruction is already satisfied.

## Values

Pass the values the step needs with `--value`. Repeat the flag for several values.

```bash
agent-browser act "Fill the email field" --value email=me@example.com
agent-browser act "Pick the shipping country" --value country=Germany
```

The decider sees the value key names, never the values. It chooses which key goes into which field; the code copies the value. With a single value there is no choice to make, so it is used directly. With several values the decider also picks the key.

## Decision status

<table>
  <thead>
    <tr><th>Status</th><th>Meaning</th></tr>
  </thead>
  <tbody>
    <tr><td><code>executed</code></td><td>The chosen action ran. <code>result</code> holds the action output.</td></tr>
    <tr><td><code>done</code></td><td>The instruction was already satisfied. Nothing ran.</td></tr>
    <tr><td><code>blocked</code></td><td>Confidence was below <code>--min-confidence</code>, or the chosen operation needs a target or value that was not offered. <code>reason</code> explains the second case. Nothing ran.</td></tr>
    <tr><td><code>stale</code></td><td>The page changed after two decisions. Nothing ran.</td></tr>
    <tr><td><code>dry_run</code></td><td>A proposal was returned and nothing ran.</td></tr>
  </tbody>
</table>

The result also includes `operation`, `target`, `valueKey`, `confidence`, `probabilities`, `fingerprint`, `candidateCount`, and `candidatesTruncated`. A blocked result includes `reason` when the chosen operation needs a target or value that was not offered.

## Candidates

Only rendered, actionable elements are offered: link, button, textbox, searchbox, combobox, checkbox, radio, option, menuitem, and tab. Elements outside the viewport are included because an action scrolls the chosen ref into view before it runs. The list keeps DOM order and stops at 80 elements. `candidatesTruncated` reports when the cap dropped any. To narrow the list, snapshot a region with `snapshot -s <selector>` or scroll first.

Unusual answers are contained. A choice outside the offered options, a probability set that does not cover the offered options, or a choice that is not the most likely option fails validation. `act` retries once, and a second invalid answer is an error; nothing runs.

## Choosing a decider

`AGENT_BROWSER_DECIDER` is required and has no default.

```bash
AGENT_BROWSER_DECIDER=jev TYPESAFE_API_KEY=... agent-browser act "Click Sign in"
AGENT_BROWSER_DECIDER=llm AI_GATEWAY_API_KEY=... agent-browser act "Click Sign in"
```

<table>
  <thead>
    <tr><th>Variable</th><th>Use</th></tr>
  </thead>
  <tbody>
    <tr><td><code>AGENT_BROWSER_DECIDER</code></td><td><code>jev</code> or <code>llm</code>.</td></tr>
    <tr><td><code>TYPESAFE_API_KEY</code></td><td>Required for <code>jev</code>.</td></tr>
    <tr><td><code>TYPESAFE_MODEL</code></td><td>Model for <code>jev</code>. Default <code>jev-latest</code>.</td></tr>
    <tr><td><code>TYPESAFE_BASE_URL</code></td><td>TypeSafe base URL. Default <code>https://api.typesafe.ai</code>.</td></tr>
    <tr><td><code>AGENT_BROWSER_DECIDER_MODEL</code></td><td>Model for <code>llm</code>. Default <code>anthropic/claude-sonnet-4.6</code>.</td></tr>
  </tbody>
</table>

`--min-confidence` applies to `jev`, whose probabilities are calibrated. The `llm` decider reports probabilities of 0 or 1, so the gate is skipped.

## Privacy

`act` sends the instruction, page URL and title, candidate role and accessible name, and value key names to the selected provider. It never sends input values, passwords, cookies, storage, auth vault entries, screenshots, or network bodies. `--allowed-domains` does not block decider traffic. See `references/trust-boundaries.md` for the egress rules.

## Examples

```bash
agent-browser act "Click the first result"
agent-browser act "Search for running shoes" --value query="running shoes"
agent-browser act "Fill the coupon field" --value coupon=SAVE10
agent-browser act "Scroll down"
agent-browser act "Click Submit" --dry-run
agent-browser act "Click Delete account" --min-confidence 0.9
```
