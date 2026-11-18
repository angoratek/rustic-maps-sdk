# Glyph protocol fixtures

Two committed ranges of the Mapbox glyph protocol, the payload a style's
`glyphs` endpoint serves. The label pass and the native vector-base install
both read them, so they live in the shared fixtures directory beside the
provider tile payloads.

- `noto_sans_regular_0-255.pbf` — one real Mapbox glyph-protocol range (the
  `glyphs` protobuf message, 76,580 bytes, SHA-256
  `ef1f38a3f1978591e846e9eaddf8a54f7047f546fc6aaed7872cc53151a5de78`),
  fetched from
  `https://tiles.openfreemap.org/fonts/Noto%20Sans%20Regular/0-255.pbf` on
  **2026-10-30**. The address is the `glyphs_url_template`
  (`https://tiles.openfreemap.org/fonts/{fontstack}/{range}.pbf`) of the
  `openfreemap` descriptor in `platform/providers.json` with the Bright
  style's font stack (`Noto Sans Regular`) and the Basic Latin range
  substituted; the OpenStreetMap US descriptor publishes the same protocol at
  `https://tiles.openstreetmap.us/fonts/{fontstack}/{range}.pbf`. The payload
  is the plain protobuf body, not gzip-wrapped.

- `noto_sans_regular_256-511.pbf` — the protocol's next range (the
  `glyphs` protobuf message, 127,219 bytes, SHA-256
  `992557f9e2574cfb9a75e16c7b52832578849772b7c9a6e480d6213fb008fe5d`),
  fetched from
  `https://tiles.openfreemap.org/fonts/Noto%20Sans%20Regular/256-511.pbf` on
  **2026-11-13**. The glyph protocol serves ranges in 256-codepoint blocks, and
  this one carries Latin Extended-A and the first half of Latin Extended-B
  (`U+0100`–`U+01FF`), so it is the second range a style's Latin labels need.

  The font stack record names the CSS fallback list the producer rendered
  (`Noto Sans Regular, Noto Naskh Arabic Regular, Noto Sans Armenian Regular,
  …`) and carries 223 glyphs in the first range; this slice consumes the Latin
  ones only. Glyphs are signed-distance-field bitmaps with a 3-pixel border,
  rendered at the protocol's 24-pixel em.

  Tests never touch the network: the rendering golden reads the first committed
  file, the hosted-range tests read both through a recording client, and the
  native install tests read both through the provider's style-resource seam.
  Map data © OpenStreetMap contributors; glyph rendering served by OpenFreeMap
  under the OpenFreeMap terms (<https://openfreemap.org>). The Noto Sans
  Regular stack is licensed under the SIL Open Font License 1.1 (Copyright
  2022 The Noto Project Authors); the licence text is committed at
  `docs/THIRD_PARTY_LICENSES/noto-sans-OFL-1.1.txt`.
