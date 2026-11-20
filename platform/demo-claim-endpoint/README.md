# Hosted demo claim endpoint

The one server-side piece of the hosted mobile demo: it decides whether a
visitor may start a streamed session, and it is the only thing standing between
a crawler and the Appetize Free plan's 30 minutes a month.

## What it enforces

| Limit | Value | Where |
|---|---|---|
| Month's streamed minutes | 30 | `MONTHLY_BUDGET_SECONDS` |
| One session's length | 3 minutes | `SESSION_SECONDS` |
| Sessions granted per UTC day, all visitors | 4 | `DAILY_GRANT_LIMIT` |
| Sessions per browser visitor token per UTC day | 2 | `PER_VISITOR_DAILY_LIMIT` |
| Sessions per client address per UTC day (backstop) | 4 | `PER_ADDRESS_DAILY_LIMIT` |

The page generates a random visitor token, keeps it in this browser and sends
it with every claim. Counting per token rather than per address is deliberate:
a VPN exit or a carrier NAT shares one address, so the old address-keyed limit
locked out visitors who had never watched a session and let one person reset
theirs by reconnecting. The address counter remains as a backstop above the
per-visitor limit, so clearing browser storage does not buy unlimited sessions;
the month's minutes stay the hard ceiling either way.

A grant reserves the **whole** session before answering, so an abandoned
session costs the budget what a completed one costs. The page never embeds a
stream before hearing `{"state":"ok"}`.

That reservation is deliberately pessimistic: a visitor who claims and leaves
without pressing Appetize's own Start button never streams, yet costs three
minutes. The endpoint therefore reconciles the month's counter with Appetize's
own usage report (`GET /v1/usageSummary`, the account key in `APPETIZE_API_KEY`)
before each decision. The report brings the counter down to what actually
streamed plus one session of headroom for claims in flight, and never below the
report or above the reservation. Without the key, or when the report is
unreachable, the reservation stands alone.

## Answers

```json
{"state":"ok","platform":"android","sessionSeconds":180,"remainingSeconds":900}
```

```json
{"state":"resting","reason":"budget","detail":"This month’s demo minutes are used up. …"}
```

`reason` is one of `maintenance` (the kill switch), `budget`, `daily`,
`visitor` or `address`. The page renders `detail` verbatim and appends the
month's remaining minutes; it never invents its own promise about when the
demo returns.

CORS answers the one origin the site is served from, echoing the caller's
origin: `https://angoratek.github.io` (what the Pages deployment publishes).

An `address` refusal is the shared-address backstop, not the normal answer.
While developing against the live endpoint, remember that a claim spends a
session's minutes even when no stream starts; the endpoint keeps its counters
in the linked store, and they can be read, and cleared for a test day,
through the Upstash REST API.

## Reading the counters

`GET /api/claim?platform=android&status=true` answers the same counters the
decision reads — `remainingSeconds`, `grantsToday`, `visitorGrantsToday`,
`addressGrantsToday` — **without** reserving a session, so a maintainer can
check why the page is resting without spending three of the month's thirty
minutes; `streamedSeconds` is the reconciled figure when the usage report was
available. Add `&visitor=<token>` to see that visitor's own count.

## Operator access

Set `DEMO_OPERATOR_TOKEN` (at least 16 characters) in the project settings, then
open the page as `/demo/mobile/?operatorToken=<token>`. An operator claim skips
the daily, visitor and address gates — so the demo never locks the owner out of
their own test while minutes remain — but it still reserves a session from the
month's budget, the one limit nothing can lift. The token is never stored in the
page or in git; the query parameter is read and forwarded only while the page is
open.

## Tests

`npm test` in this directory (or `node --test api`) runs both suites:
`claim_limits.test.mjs` covers the pure decision (the budget first, the shared
day, the visitor, the address backstop, the operator bypass that still respects
the month, and the reconciliation arithmetic), and `appetize_usage.test.mjs`
covers the usage report's month key, per-app summing, rounding and failure.

## Environment

| Variable | Source | Purpose |
|---|---|---|
| `KV_REST_API_URL`, `KV_REST_API_TOKEN` | injected by the linked Vercel KV store | the counters |
| `DEMO_KILL_SWITCH` | set to `true` in the project settings | stops every grant immediately |
| `DEMO_OPERATOR_TOKEN` | a random secret of at least 16 characters | lets the owner watch while minutes remain |
| `APPETIZE_API_KEY` | the account API key | reads the month's usage report; optional — without it the reservation stands alone |

The upload token is deliberately **not** here: uploads are a maintainer and CI
action (`scripts/upload-demo-builds.mjs`), and the page only ever holds a public
build id. `APPETIZE_API_KEY` is read-only usage reporting and never reaches the
page.

## The store, and what it costs

Vercel's own KV product is retired; the CLI provisions **Upstash for Redis**
through the marketplace. The endpoint runs on a **Free** Upstash store
connected to both projects, which is supported and costs nothing. The
counters use their own key namespace, so the two applications never read each
other's keys.

```sh
# What exists
vercel integration list                          # the resource and its projects
vercel env ls                                    # KV_REST_API_URL / KV_REST_API_TOKEN

# Connect an existing store to a project (API; the CLI has no connect verb)
vercel api /v1/integrations/installations/<icfg_…>/resources/<store_…>/connections \
  -X POST --input - <<'JSON'
{"projectId":"<prj_…>"}
JSON
```

A new store cannot be created on the free plan from the CLI: `POST
/v1/storage/stores/integration/direct` answers *"No free billing plans are
available for product 'upstash-kv' in this integration. Use billingPlanId
parameter for paid plans."* — so the free tier comes from an existing store on
the account (as here) or from Upstash directly, with its REST URL and token set
as the two `KV_REST_API_*` variables.

### Cost guardrails

- The store is on the **Free** plan: no subscription and no payment method
  attached, so the worst case is the plan's own quota, not a bill.
- `autoUpgrade` is **off**, so the resource can never move itself to a paid
  plan, and `eviction` is off, so a full database refuses writes rather than
  silently dropping counters (a failed reservation is a refusal, never an
  unmetered grant).
- The endpoint's own budget is the real ceiling: 30 minutes a month, four
  grants a day across visitors, and the kill switch stops it immediately.

## Deploying

```sh
cd platform/demo-claim-endpoint
vercel link                       # create or attach the project
vercel env pull                   # KV_REST_API_URL / KV_REST_API_TOKEN
vercel deploy --prod
```

Production answers at `https://rustic-maps-demo-claim.vercel.app/api/claim`,
which is the `demo-claim-endpoint` meta the `/demo/mobile` page reads. The
function refuses to count without its store, which is the honest failure — it
answers an error rather than granting unmetered sessions.

## Why not client-side

A browser-side counter resets with the tab, the profile or the cache, and a
crawler reloads. The month's minutes are shared, so their counter has to be
shared too.
