#!/usr/bin/env node
// Converts the committed data-only vector style artifacts into the version-8
// style documents `rustic-maps-style` compiles. The artifacts under
// fixtures/render/ stay in place as the converter's input and as the legacy
// parser's parity fixture; the generated documents live beside them under
// fixtures/render/styles/.
//
// The converter is deterministic: it reads each artifact, builds the document
// in a fixed key order, and writes two-space JSON with a trailing newline.
// Re-running it on an unchanged artifact produces byte-identical output, which
// `--check` asserts without writing.
//
// Mapping rules (mirroring the legacy renderer model):
// - the artifact's `background` becomes one `background` layer;
// - `source_layer` becomes `source: "base"` plus `source-layer`;
// - `geometry_kind` becomes the layer `type`;
// - `rgba`, `text_rgba` and `halo_rgba` become colour properties whose channels
//   are the artifact's sRGB components scaled to 0..=255;
// - the one-attribute `filter` becomes equality expressions (a filter with no
//   alternative matches nothing, as the legacy matcher did);
// - `width_zoom_stops` and `text_size_zoom_stops` become linear `interpolate`
//   expressions on `["zoom"]`;
// - `rank_zoom_stops` and `class_minimum_zooms` become nested `step`/`case`
//   gates inside the layer filter, because neither has a direct property;
// - symbol fields map to `text-field` fallbacks, `text-size`, `text-color`,
//   `text-halo-color`, `text-halo-width`, `symbol-placement`,
//   `symbol-sort-key` and `text-allow-overlap`.
//
// Usage: node scripts/convert-vector-style-artifacts.mjs [--check]

import {
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = dirname(scriptDirectory);
const artifactDirectory = join(
  repositoryRoot,
  "crates/rustic-maps-rendering/fixtures/render",
);
const outputDirectory = join(artifactDirectory, "styles");

/// The style artifact basenames the renderer loads, in style-id order.
const ARTIFACT_FILE_NAMES = [
  "vector-style.json",
  "vector-style-dark.json",
  "vector-style-openmaptiles-light.json",
  "vector-style-openmaptiles-dark.json",
  "vector-style-bright-light.json",
  "vector-style-fiord-dark.json",
  "vector-style-positron-light.json",
];

/// The legacy renderer's width default when a line entry omits one.
const DEFAULT_LINE_WIDTH_PIXELS = 1.5;
/// The legacy renderer's circle radius default.
const DEFAULT_CIRCLE_RADIUS_PIXELS = 3.0;
/// The legacy renderer's text size default.
const DEFAULT_TEXT_SIZE_PIXELS = 12.0;
/// The legacy renderer's halo width default.
const DEFAULT_HALO_WIDTH_PIXELS = 1.0;
/// The legacy renderer's text-field fallbacks when an entry omits them.
const DEFAULT_TEXT_FIELD_FALLBACKS = ["name:latin", "name", "ref"];
/// The legacy renderer's single-font stack default.
const DEFAULT_FONT_STACK = ["Noto Sans Regular"];
/// The largest value a 32-bit unsigned rank can carry.
const MAXIMUM_U32 = 4294967295;
/// The sRGB channel range the style specification's colour arrays use.
const COLOR_CHANNEL_MAXIMUM = 255;

/// Scales the artifact's 0..1 sRGB channels to the 0..=255 array form the
/// style specification uses; the alpha channel stays 0..=1 because the parser
/// reads it as a fraction.
const colorValue = (rgba) => [
  rgba[0] * COLOR_CHANNEL_MAXIMUM,
  rgba[1] * COLOR_CHANNEL_MAXIMUM,
  rgba[2] * COLOR_CHANNEL_MAXIMUM,
  rgba[3],
];

/// A linear zoom interpolation from the artifact's stops, or the fixed value
/// when the entry carries no stops.
const zoomInterpolation = (stops, valueOf, fallback) => {
  if (!stops || stops.length === 0) {
    return fallback;
  }
  const expression = ["interpolate", ["linear"], ["zoom"]];
  for (const stop of stops) {
    expression.push(stop.zoom, valueOf(stop));
  }
  return expression;
};

/// The legacy one-attribute filter as equality expressions. A filter whose
/// every alternative is empty matches no feature, exactly like the legacy
/// matcher.
const filterExpression = (filter) => {
  if (!filter) {
    return null;
  }
  const conditions = [];
  if (filter.expected_text) {
    conditions.push(["==", ["get", filter.attribute_name], filter.expected_text]);
  }
  for (const alternative of filter.matches_any ?? []) {
    conditions.push(["==", ["get", filter.attribute_name], alternative]);
  }
  if (filter.number_equals !== undefined && filter.number_equals !== null) {
    conditions.push(["==", ["get", filter.attribute_name], filter.number_equals]);
  }
  if (conditions.length === 0) {
    return false;
  }
  return conditions.length === 1 ? conditions[0] : ["all", ...conditions];
};

/// The per-class minimum zooms as a `case` of `step` gates on `["zoom"]`.
/// A class named by several groups takes the highest minimum, matching the
/// legacy gate; a feature without a matching class falls through to `true`.
const classGate = (groups) => {
  if (!groups || groups.length === 0) {
    return null;
  }
  const minimumByClass = new Map();
  for (const group of groups) {
    for (const featureClass of group.classes) {
      minimumByClass.set(
        featureClass,
        Math.max(
          minimumByClass.get(featureClass) ?? Number.NEGATIVE_INFINITY,
          group.min_zoom,
        ),
      );
    }
  }
  const classesByMinimum = new Map();
  for (const featureClass of [...minimumByClass.keys()].sort()) {
    const minimum = minimumByClass.get(featureClass);
    const classes = classesByMinimum.get(minimum) ?? [];
    classes.push(featureClass);
    classesByMinimum.set(minimum, classes);
  }
  const arms = ["case"];
  for (const minimum of [...classesByMinimum.keys()].sort((a, b) => a - b)) {
    arms.push(
      ["in", ["get", "class"], ["literal", classesByMinimum.get(minimum)]],
      ["step", ["zoom"], false, minimum, true],
    );
  }
  arms.push(true);
  return arms;
};

/// The feature-rank ceiling as a stepped `["zoom"]` gate. Rank is ordinal, so
/// the ceiling steps rather than interpolates, and a feature without a rank
/// carries the legacy maximum so it is only filtered when the ceiling is not.
const rankGate = (stops) => {
  if (!stops || stops.length === 0) {
    return null;
  }
  const ceiling = ["step", ["zoom"], stops[0].maximum_rank];
  for (let index = 1; index < stops.length; index += 1) {
    ceiling.push(stops[index].zoom, stops[index].maximum_rank);
  }
  const rank = [
    "max",
    0,
    [
      "min",
      MAXIMUM_U32,
      ["floor", ["coalesce", ["get", "rank"], MAXIMUM_U32]],
    ],
  ];
  return ["<=", rank, ceiling];
};

/// Combines the layer's data filter with its zoom gates.
const combinedFilter = (artifactLayer) => {
  const parts = [
    filterExpression(artifactLayer.filter),
    classGate(artifactLayer.class_minimum_zooms),
    rankGate(artifactLayer.rank_zoom_stops),
  ].filter((part) => part !== null);
  if (parts.length === 0) {
    return null;
  }
  return parts.length === 1 ? parts[0] : ["all", ...parts];
};

/// The ordered text fallbacks as a `case` chain: the first fallback that the
/// feature carries as a non-empty string wins, and no match yields "".
const textFieldExpression = (fallbacks) => {
  let expression = "";
  for (let index = fallbacks.length - 1; index >= 0; index -= 1) {
    const key = fallbacks[index];
    expression = [
      "case",
      [
        "all",
        ["has", key],
        ["!=", ["get", key], ""],
        ["==", ["get", key], ["to-string", ["get", key]]],
      ],
      ["get", key],
      expression,
    ];
  }
  return expression;
};

/// The `paint` object of one artifact layer.
const paintObject = (artifactLayer) => {
  switch (artifactLayer.geometry_kind) {
    case "fill":
      return {
        "fill-color": colorValue(artifactLayer.rgba ?? [0, 0, 0, 1]),
        "fill-opacity": 1,
      };
    case "line":
      return {
        "line-color": colorValue(artifactLayer.rgba ?? [0, 0, 0, 1]),
        "line-width": zoomInterpolation(
          artifactLayer.width_zoom_stops,
          (stop) => stop.width_pixels,
          artifactLayer.width_pixels ?? DEFAULT_LINE_WIDTH_PIXELS,
        ),
        "line-opacity": 1,
      };
    case "circle":
      return {
        "circle-color": colorValue(artifactLayer.rgba ?? [0, 0, 0, 1]),
        "circle-radius":
          artifactLayer.radius_pixels ?? DEFAULT_CIRCLE_RADIUS_PIXELS,
        "circle-opacity": 1,
      };
    case "symbol":
      return {
        "text-color": colorValue(artifactLayer.text_rgba ?? [0, 0, 0, 1]),
        "text-halo-color": colorValue(
          artifactLayer.halo_rgba ?? [1, 1, 1, 1],
        ),
        "text-halo-width":
          artifactLayer.halo_width_pixels ?? DEFAULT_HALO_WIDTH_PIXELS,
      };
    default:
      throw new Error(
        `unknown artifact geometry_kind ${artifactLayer.geometry_kind}`,
      );
  }
};

/// The `layout` object of one artifact symbol layer.
const layoutObject = (artifactLayer) => {
  const textFieldFallbacks =
    artifactLayer.text_field_fallbacks ?? DEFAULT_TEXT_FIELD_FALLBACKS;
  const fontStack = artifactLayer.font_stack ?? DEFAULT_FONT_STACK;
  return {
    "text-field": textFieldExpression(textFieldFallbacks),
    "text-font": fontStack,
    "text-size": zoomInterpolation(
      artifactLayer.text_size_zoom_stops,
      (stop) => stop.size_pixels,
      artifactLayer.text_size_pixels ?? DEFAULT_TEXT_SIZE_PIXELS,
    ),
    "symbol-placement": artifactLayer.placement ?? "point",
    "symbol-sort-key": artifactLayer.priority ?? 0,
    "text-allow-overlap": artifactLayer.allow_overlap ?? false,
  };
};

/// One artifact layer as a version-8 style layer.
const styleLayer = (artifactLayer, styleName, index) => {
  const layer = {
    id: `${styleName}-${index}`,
    type: artifactLayer.geometry_kind,
    source: "base",
    "source-layer": artifactLayer.source_layer,
  };
  if (artifactLayer.min_zoom !== undefined) {
    layer.minzoom = artifactLayer.min_zoom;
  }
  if (artifactLayer.max_zoom !== undefined) {
    layer.maxzoom = artifactLayer.max_zoom;
  }
  const filter = combinedFilter(artifactLayer);
  if (filter !== null) {
    layer.filter = filter;
  }
  if (artifactLayer.geometry_kind === "symbol") {
    layer.layout = layoutObject(artifactLayer);
  }
  layer.paint = paintObject(artifactLayer);
  return layer;
};

/// One artifact as a complete version-8 document.
const styleDocument = (artifact, sourceFileName) => {
  const styleName = basename(sourceFileName, ".json");
  const layers = [];
  if (artifact.background) {
    layers.push({
      id: "background",
      type: "background",
      paint: { "background-color": colorValue(artifact.background) },
    });
  }
  artifact.layers.forEach((artifactLayer, index) => {
    layers.push(styleLayer(artifactLayer, styleName, index));
  });
  return {
    version: 8,
    name: styleName,
    metadata: { source_artifact: sourceFileName },
    sources: { base: { type: "vector" } },
    layers,
  };
};

const main = () => {
  const checkOnly = process.argv.includes("--check");
  if (!checkOnly) {
    mkdirSync(outputDirectory, { recursive: true });
  }
  let mismatchedFileCount = 0;
  for (const sourceFileName of ARTIFACT_FILE_NAMES) {
    const sourcePath = join(artifactDirectory, sourceFileName);
    if (!existsSync(sourcePath)) {
      throw new Error(`the style artifact ${sourcePath} is missing`);
    }
    const artifact = JSON.parse(readFileSync(sourcePath, "utf8"));
    const document = styleDocument(artifact, sourceFileName);
    const output = `${JSON.stringify(document, null, 2)}\n`;
    const outputPath = join(outputDirectory, sourceFileName);
    if (checkOnly) {
      const existing = existsSync(outputPath)
        ? readFileSync(outputPath, "utf8")
        : "";
      if (existing !== output) {
        mismatchedFileCount += 1;
        console.error(`${sourceFileName}: generated document is out of date`);
      }
    } else {
      writeFileSync(outputPath, output);
    }
  }
  if (mismatchedFileCount > 0) {
    process.exitCode = 1;
  }
};

main();
