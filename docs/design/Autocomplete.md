# Type-ahead search

**What the reader gets:** the rule every reference application follows when a
search field offers suggestions while the user types, and the shared values
that rule depends on.

## The rule

1. **Three characters.** A field asks the provider only when the trimmed query
   holds at least three characters (`autocomplete-minimum-query-characters`).
   A shorter query clears the suggestion list and shows nothing.
2. **250 milliseconds.** Each keystroke restarts a 250-millisecond quiet timer
   (`autocomplete-debounce-milliseconds`). Only the query still current when
   the timer fires may ask the provider.
3. **Five rows.** A field shows at most five suggestions
   (`autocomplete-maximum-results`); the provider asks for the same number.
4. **One request in flight.** A new query cancels the previous request before
   starting its own. A late answer from a superseded query is dropped, never
   shown. The rule is per field: each field owns its list, so asking in one
   never cancels the other.
5. **Selection fills the field.** Choosing a suggestion puts its display line
   in the field, closes the list, and holds the chosen place so the next
   action (plan, or the search field's own submit) resolves it directly —
   the same outcome as typed text that matched a place.
6. **No network in tests.** Suggestions in every test come from committed
   fixtures served same-origin.

## Which provider answers

The settings surface's provider rows decide, per credential state:

- With a platform credential configured, the credentialed provider serves the
  type-ahead (Search Ahead for MapQuest).
- Without a credential, the keyless search provider serves it (Photon), so the
  demo offers suggestions before any key is entered.

A shell never names a provider: both come from the shared registry.

## Empty and error states

- Nothing typed, or fewer than three characters: no list, no request.
- A provider failure: the field keeps its text and shows one inline sentence;
  a later keystroke retries. A cancelled request shows nothing.
- No matches: one "no matches" row, not an empty list.

## Verification

Each target asserts the same five facts: the three-character threshold, the
debounce window, the five-row cap, cancellation of a superseded request, and
selection filling the field. The browser suite drives a fixture route; the
Android, iOS and React Native suites drive their own fields against the same
committed responses.
