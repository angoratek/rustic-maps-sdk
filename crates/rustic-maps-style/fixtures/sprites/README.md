# Sprite fixtures

## `icon-sprite.json` and `icon-sprite.png`

A four-icon sprite atlas drawn for the icon-rendering tests: a blue railway
disc, a green bus square, a green park triangle and a cyan water diamond, each
16 by 16 pixels on a 64 by 16 sheet at a pixel ratio of one. The artwork is
generated from geometric primitives inside this repository and carries no
third-party asset or licence.

The index follows the MapLibre sprite-index format (`x`, `y`, `width`,
`height`, `pixelRatio`). Tests resolve the style's `fixture://icon-sprite`
sprite URL through an injected in-memory source that reads these two files; no
test touches the network.
