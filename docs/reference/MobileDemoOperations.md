# Hosted mobile demo operations

The mobile demo at `/demo/mobile/` streams the Android, iOS and React Native
reference applications through Appetize. This page records how the free plan
is used, what visitors see, and what the owner can do when a session is
needed for review.

## What the free plan allows

- **30 streamed minutes per month**, shared by every visitor.
- **3-minute sessions**, so at most ten full sessions a month.
- Sessions open in their own browser tab; the free plan cannot embed a build,
  so Appetize's own page may show an "app may not be embedded" notice when a
  link has embed parameters. Opening the session top-level is unaffected.
- Nothing streams until the visitor presses **Start demo** on the hosted page.

## The claim service

`platform/web/mobile-demo/mobile-demo.mjs` asks a claim endpoint for a
session. The endpoint enforces:

- the monthly streamed-minutes budget,
- a per-visitor daily session count (the visitor is a randomly generated local
  token, not an IP address, so shared addresses do not lock each other out),
- an optional operator bypass through `?operatorToken=` on the demo page.

The page never holds the operator secret; the token travels only in the URL
the owner opens. When the daily count is reached the page rests with
"You have used today's demo sessions. Try again tomorrow." and the month's
remaining minutes.

## Reviewing the demo yourself

Open the demo page with the operator token appended:

```
https://<demo-host>/demo/mobile/?operatorToken=<operator-token>
```

The claim endpoint grants the session without counting against the daily
visitor allowance; the streamed minutes still count against the month. Use it
for review, screenshots and support checks.

## When the month runs out

The page rests until the claim service's month rolls over. Raising the
budget needs an Appetize plan change, which is an owner account action.

## Re-applying the session settings

The uploader updates settings only when it uploads. To re-apply them to the
published builds without a new artifact (for example after an account-level
change dropped the referrer allow-list):

```bash
set -a; source .env; set +a
node scripts/upload-demo-builds.mjs --reapply-settings
```

It reads each build id from `platform/demo-builds.json`, sends the timeout,
three-minute limit, one-session concurrency, referrer allow-list and button
text as a JSON body, and verifies the allow-list reads back. The builds and
their ids are unchanged.

## Rebuilding the hosted builds

The uploader (`scripts/upload-demo-builds.mjs`) updates the three published
Appetize builds in place (Android, iOS and React Native Android; a React Native
iOS entry exists but is unpublished, so it has no build id). Build ids never
change. The Android hosted build may carry the
demo's dedicated provider credential, injected on the build machine from
`HOSTED_DEMO_PROVIDER_API_KEY`; no other build does. Re-upload after any
change that should reach visitors, then confirm the app starts in a session.

## Owner actions

- Keep the Appetize app allow-list current if embedding is ever revisited.
- Rotate the demo's dedicated provider key on the recorded schedule.
- Watch the monthly minutes around a launch; the budget is shared.
