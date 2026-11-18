# Mobile demo

<div class="demo-launch">

<a href="demo/mobile/">Open the mobile demo</a>

</div>

Run the Android, iOS, and React Native reference applications on a hosted
device. Choose a platform and press **Start demo**; the session opens in its
own browser tab for three minutes.

## What the session is

The builds are the same reference applications the guides walk through: the
map view, search and route planning, turn-by-turn simulation, the offline
card, and the settings sheet. The hosted build may carry a dedicated provider
credential issued for this demo alone. No visitor credential is involved, and
the packages you download or install never carry one.

If a build cannot create a rendering surface on the hosted device, the app
shows its own failure state instead of a blank frame.

## The monthly budget

The demo runs on a free plan with 30 streamed minutes a month, shared by every
visitor. Nothing streams until you press Start, and a session lasts three
minutes. When the month's minutes are gone, the page says so and the demo
rests until the month turns over.

The [browser demo](demo/) needs no session and no monthly budget. It runs the
same SDK in your own browser, and the mobile page links to it whenever the
hosted demo rests.

## The daily limit

Each visitor gets a limited number of sessions per day. When you have used
today's sessions, the page asks you to come back tomorrow. The daily limit
keeps one visitor from spending the shared monthly budget.

A refusal leaves the Start button in place, so a failure that turns out to be
transient can be retried without reloading the page.
