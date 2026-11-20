// Authors the OpenStreetMap US vector-tile fixtures from a declarative scene.
//
// The tiles this script writes are synthetic: no tile is downloaded, and the
// geometry carries no OpenStreetMap data. They reproduce the Sourdough layer
// schema the consuming tests assert on, so the decode, styling, golden and
// performance paths stay exercised without redistributing a tile from the
// OpenStreetMap US service, whose usage policy prohibits it.
//
// The bytes are a Mapbox Vector Tile protobuf encoded directly by the small
// writer below, so the fixtures are reproducible with Node alone:
//
//     node scripts/generate-openstreetmap-us-fixtures.mjs
//
// Two canonical fixtures are written, plus the z7 and z8 performance copies
// the browser fit-budget harness serves. The script prints each file's byte
// count and SHA-256 so `fixtures/provenance.json` can be refreshed.

import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** The repository root, resolved from this script's location. */
const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

/** The canonical synthetic fixture directory. */
const CANONICAL_FIXTURE_DIRECTORY = path.join(
  REPOSITORY_ROOT,
  'fixtures',
  'openstreetmap-us',
);

/** The performance harness's own fixture directory. */
const PERFORMANCE_FIXTURE_DIRECTORY = path.join(
  REPOSITORY_ROOT,
  'platform',
  'web',
  'perf',
  'fixtures',
  'vector',
);

/** The tile-local coordinate extent every layer declares. */
const TILE_EXTENT = 4096;

/** The Mapbox Vector Tile layer version every layer declares. */
const TILE_VERSION = 2;

/** Protobuf wire types this writer uses. */
const WIRE_TYPE_VARINT = 0;
const WIRE_TYPE_LENGTH_DELIMITED = 2;

/** Geometry type numbers from the MVT specification. */
const GEOMETRY_TYPE_POINT = 1;
const GEOMETRY_TYPE_LINE_STRING = 2;
const GEOMETRY_TYPE_POLYGON = 3;

/** Geometry command ids from the MVT specification. */
const COMMAND_MOVE_TO = 1;
const COMMAND_LINE_TO = 2;
const COMMAND_CLOSE_PATH = 7;

/** ZigZag-encodes one signed geometry parameter. */
function zigzagEncode(value) {
  return ((value << 1) ^ (value >> 31)) >>> 0;
}

/** Appends one base-128 varint. */
function appendVarint(value, bytes) {
  let rest = value >>> 0;
  while (rest >= 0x80) {
    bytes.push((rest & 0x7f) | 0x80);
    rest >>>= 7;
  }
  bytes.push(rest);
}

/** Appends a field key. */
function appendFieldKey(fieldNumber, wireType, bytes) {
  appendVarint(((fieldNumber << 3) | wireType) >>> 0, bytes);
}

/** Appends a length-delimited field. */
function appendLengthDelimitedField(fieldNumber, payload, bytes) {
  appendFieldKey(fieldNumber, WIRE_TYPE_LENGTH_DELIMITED, bytes);
  appendVarint(payload.length, bytes);
  bytes.push(...payload);
}

/** Appends a varint field. */
function appendVarintField(fieldNumber, value, bytes) {
  appendFieldKey(fieldNumber, WIRE_TYPE_VARINT, bytes);
  appendVarint(value, bytes);
}

/** Appends a packed repeated varint field. */
function appendPackedVarintField(fieldNumber, values, bytes) {
  const payload = [];
  for (const value of values) {
    appendVarint(value, payload);
  }
  appendLengthDelimitedField(fieldNumber, payload, bytes);
}

/** Packs one geometry command integer from its id and count. */
function packCommandInteger(commandId, commandCount) {
  return ((commandCount << 3) | commandId) >>> 0;
}

/** Builds the command stream of one set of points. */
function pointCommands(points) {
  const commands = [packCommandInteger(COMMAND_MOVE_TO, points.length)];
  let cursorX = 0;
  let cursorY = 0;
  for (const [xCoordinate, yCoordinate] of points) {
    commands.push(zigzagEncode(xCoordinate - cursorX));
    commands.push(zigzagEncode(yCoordinate - cursorY));
    cursorX = xCoordinate;
    cursorY = yCoordinate;
  }
  return commands;
}

/** Builds the command stream of one line string. */
function lineCommands(points) {
  const [firstPoint, ...rest] = points;
  const commands = [
    packCommandInteger(COMMAND_MOVE_TO, 1),
    zigzagEncode(firstPoint[0]),
    zigzagEncode(firstPoint[1]),
  ];
  if (rest.length > 0) {
    commands.push(packCommandInteger(COMMAND_LINE_TO, rest.length));
  }
  let cursorX = firstPoint[0];
  let cursorY = firstPoint[1];
  for (const [xCoordinate, yCoordinate] of rest) {
    commands.push(zigzagEncode(xCoordinate - cursorX));
    commands.push(zigzagEncode(yCoordinate - cursorY));
    cursorX = xCoordinate;
    cursorY = yCoordinate;
  }
  return commands;
}

/** Builds the command stream of one polygon ring set. */
function polygonCommands(rings) {
  const commands = [];
  let cursorX = 0;
  let cursorY = 0;
  for (const ring of rings) {
    const [firstPoint, ...rest] = ring;
    commands.push(packCommandInteger(COMMAND_MOVE_TO, 1));
    commands.push(zigzagEncode(firstPoint[0] - cursorX));
    commands.push(zigzagEncode(firstPoint[1] - cursorY));
    cursorX = firstPoint[0];
    cursorY = firstPoint[1];
    if (rest.length > 0) {
      commands.push(packCommandInteger(COMMAND_LINE_TO, rest.length));
    }
    for (const [xCoordinate, yCoordinate] of rest) {
      commands.push(zigzagEncode(xCoordinate - cursorX));
      commands.push(zigzagEncode(yCoordinate - cursorY));
      cursorX = xCoordinate;
      cursorY = yCoordinate;
    }
    commands.push(packCommandInteger(COMMAND_CLOSE_PATH, 1));
  }
  return commands;
}

/** Encodes one text value message. */
function encodeTextValue(text) {
  const bytes = [];
  appendLengthDelimitedField(1, [...Buffer.from(text, 'utf8')], bytes);
  return bytes;
}

/** Encodes one feature message. */
function encodeFeature(feature, keyIndexByKey, valueIndexByValue) {
  const bytes = [];
  if (feature.identifier !== undefined) {
    appendVarintField(1, feature.identifier, bytes);
  }
  const tags = [];
  for (const [key, value] of Object.entries(feature.properties ?? {})) {
    tags.push(keyIndexByKey.get(key), valueIndexByValue.get(String(value)));
  }
  if (tags.length > 0) {
    appendPackedVarintField(2, tags, bytes);
  }
  appendVarintField(3, feature.geometryType, bytes);
  appendPackedVarintField(4, feature.geometryCommands, bytes);
  return bytes;
}

/** Encodes one layer message from its declarative features. */
function encodeLayer(layer) {
  const keys = [];
  const values = [];
  const keyIndexByKey = new Map();
  const valueIndexByValue = new Map();
  for (const feature of layer.features) {
    for (const [key, value] of Object.entries(feature.properties ?? {})) {
      if (!keyIndexByKey.has(key)) {
        keyIndexByKey.set(key, keys.length);
        keys.push(key);
      }
      const textValue = String(value);
      if (!valueIndexByValue.has(textValue)) {
        valueIndexByValue.set(textValue, values.length);
        values.push(textValue);
      }
    }
  }
  const bytes = [];
  appendLengthDelimitedField(1, [...Buffer.from(layer.name, 'utf8')], bytes);
  for (const feature of layer.features) {
    appendLengthDelimitedField(
      2,
      encodeFeature(feature, keyIndexByKey, valueIndexByValue),
      bytes,
    );
  }
  for (const key of keys) {
    appendLengthDelimitedField(3, [...Buffer.from(key, 'utf8')], bytes);
  }
  for (const value of values) {
    appendLengthDelimitedField(4, encodeTextValue(value), bytes);
  }
  appendVarintField(5, TILE_EXTENT, bytes);
  appendVarintField(15, TILE_VERSION, bytes);
  return bytes;
}

/** Encodes one tile message from its layers. */
function encodeTile(layers) {
  const bytes = [];
  for (const layer of layers) {
    appendLengthDelimitedField(3, encodeLayer(layer), bytes);
  }
  return Uint8Array.from(bytes);
}

/** A point feature at one tile-local coordinate. */
function pointFeature(coordinates, properties) {
  return {
    geometryType: GEOMETRY_TYPE_POINT,
    geometryCommands: pointCommands([coordinates]),
    properties,
  };
}

/** A line-string feature through the given tile-local coordinates. */
function lineFeature(coordinates, properties) {
  return {
    geometryType: GEOMETRY_TYPE_LINE_STRING,
    geometryCommands: lineCommands(coordinates),
    properties,
  };
}

/** A polygon feature with an exterior ring and any interior rings. */
function polygonFeature(rings, properties) {
  return {
    geometryType: GEOMETRY_TYPE_POLYGON,
    geometryCommands: polygonCommands(rings),
    properties,
  };
}

/** A clockwise (y-down) rectangle ring from two opposite corners. */
function rectangleRing(minimumX, minimumY, maximumX, maximumY) {
  return [
    [minimumX, minimumY],
    [maximumX, minimumY],
    [maximumX, maximumY],
    [minimumX, maximumY],
  ];
}

/**
 * A clockwise rectangle whose four edges carry intermediate vertices that
 * deviate by less than the low-zoom simplification tolerance, so the
 * performance gate can prove that simplification removes them.
 */
function jitteredRectangleRing(
  minimumX,
  minimumY,
  maximumX,
  maximumY,
  verticesPerEdge,
  deviation,
) {
  const corners = [
    [minimumX, minimumY],
    [maximumX, minimumY],
    [maximumX, maximumY],
    [minimumX, maximumY],
  ];
  const ring = [];
  for (let cornerIndex = 0; cornerIndex < corners.length; cornerIndex += 1) {
    const start = corners[cornerIndex];
    const end = corners[(cornerIndex + 1) % corners.length];
    const deltaX = end[0] - start[0];
    const deltaY = end[1] - start[1];
    const length = Math.hypot(deltaX, deltaY);
    // The unit normal of the edge, used to push an interior vertex off the
    // straight segment by the small alternating deviation.
    const normalX = -deltaY / length;
    const normalY = deltaX / length;
    for (let vertexIndex = 0; vertexIndex < verticesPerEdge; vertexIndex += 1) {
      const fraction = vertexIndex / verticesPerEdge;
      const sign = vertexIndex % 2 === 0 ? 1 : -1;
      ring.push([
        Math.round(start[0] + deltaX * fraction + normalX * deviation * sign),
        Math.round(start[1] + deltaY * fraction + normalY * deviation * sign),
      ]);
    }
  }
  return ring;
}

/**
 * A straight line with intermediate vertices that deviate by less than the
 * low-zoom simplification tolerance.
 */
function jitteredLine(
  startX,
  startY,
  endX,
  endY,
  intermediateVertices,
  deviation,
) {
  const deltaX = endX - startX;
  const deltaY = endY - startY;
  const length = Math.hypot(deltaX, deltaY);
  const normalX = -deltaY / length;
  const normalY = deltaX / length;
  const coordinates = [[startX, startY]];
  for (let vertexIndex = 1; vertexIndex < intermediateVertices; vertexIndex += 1) {
    const fraction = vertexIndex / intermediateVertices;
    const sign = vertexIndex % 2 === 0 ? 1 : -1;
    coordinates.push([
      Math.round(startX + deltaX * fraction + normalX * deviation * sign),
      Math.round(startY + deltaY * fraction + normalY * deviation * sign),
    ]);
  }
  coordinates.push([endX, endY]);
  return coordinates;
}

/**
 * The z6 decode fixture: the eleven Sourdough-schema layers the vector-tile
 * tests pin, one feature each, in wire order.
 */
const SOURDOUGH_Z6_LAYERS = [
  {
    name: 'aeroways',
    features: [lineFeature([[600, 600], [1500, 800]], { class: 'runway' })],
  },
  {
    name: 'boundaries',
    features: [lineFeature([[400, 2048], [3600, 2048]], { boundary: 'administrative' })],
  },
  {
    name: 'highways',
    features: [
      lineFeature([[500, 1500], [3500, 1600]], {
        highway: 'motorway',
        name: 'Synthetic Motorway',
      }),
    ],
  },
  {
    name: 'landuse',
    features: [
      polygonFeature([rectangleRing(900, 900, 2200, 1300)], { landuse: 'residential' }),
    ],
  },
  {
    name: 'leisure',
    features: [
      polygonFeature([rectangleRing(1500, 2200, 2600, 2800)], { leisure: 'park' }),
    ],
  },
  {
    name: 'natural',
    features: [
      polygonFeature([rectangleRing(2200, 1200, 3300, 2000)], { natural: 'wood' }),
    ],
  },
  {
    name: 'places',
    features: [pointFeature([2048, 2048], { name: 'Synthetic Place', class: 'city' })],
  },
  {
    name: 'power',
    features: [lineFeature([[700, 2500], [1400, 2900]], { power: 'line' })],
  },
  {
    name: 'railways',
    features: [lineFeature([[1000, 3200], [3200, 3000]], { railway: 'rail' })],
  },
  {
    name: 'routes',
    features: [lineFeature([[800, 1800], [3200, 2000]], { ref: '80' })],
  },
  {
    name: 'water',
    features: [
      // The exterior ring runs clockwise in the y-down tile plane; the
      // reversed interior ring runs counter-clockwise, so the decoder reads
      // it as a hole and the hole-subtraction path stays exercised.
      polygonFeature(
        [
          rectangleRing(1200, 1500, 1900, 2100),
          rectangleRing(1400, 1700, 1650, 1900).reverse(),
        ],
        { class: 'lake' },
      ),
    ],
  },
];

/**
 * The z9 regional fixture: the same Sourdough layer vocabulary over the
 * demo's region. The routes layer carries a road ref and a transit ref so the
 * Carto shield path places both; the water and motorway geometry carries
 * redundant vertices so the low-zoom performance gate has something to
 * simplify.
 */
const SOURDOUGH_Z9_LAYERS = [
  {
    name: 'aeroways',
    features: [lineFeature([[800, 700], [1800, 1000]], { class: 'runway' })],
  },
  {
    name: 'boundaries',
    features: [lineFeature([[300, 2300], [3800, 2300]], { boundary: 'administrative' })],
  },
  {
    name: 'highways',
    features: [
      lineFeature(
        jitteredLine(400, 1400, 3700, 1500, 48, 8),
        { highway: 'motorway', name: 'Synthetic Motorway' },
      ),
      lineFeature([[500, 3200], [3600, 3100]], {
        highway: 'secondary',
        name: 'Synthetic Secondary Road',
      }),
    ],
  },
  {
    name: 'landuse',
    features: [
      polygonFeature([rectangleRing(700, 2500, 2200, 3300)], { landuse: 'residential' }),
    ],
  },
  {
    name: 'leisure',
    features: [
      polygonFeature([rectangleRing(2300, 2500, 3400, 3200)], { leisure: 'park' }),
    ],
  },
  {
    name: 'natural',
    features: [
      polygonFeature([rectangleRing(2500, 600, 3600, 1400)], { natural: 'wood' }),
    ],
  },
  {
    name: 'places',
    features: [
      pointFeature([2048, 2048], { name: 'Sacramento', class: 'city' }),
      pointFeature([3000, 2800], { name: 'West Sacramento', class: 'town' }),
    ],
  },
  {
    name: 'power',
    features: [lineFeature([[600, 3000], [1300, 3400]], { power: 'line' })],
  },
  {
    name: 'railways',
    features: [lineFeature([[900, 900], [3300, 1900]], { railway: 'rail' })],
  },
  {
    name: 'routes',
    features: [
      lineFeature([[500, 1750], [3600, 1850]], { ref: '80' }),
      lineFeature([[500, 2700], [3600, 2600]], { ref: 'Gold' }),
    ],
  },
  {
    name: 'water',
    features: [
      polygonFeature([jitteredRectangleRing(500, 2100, 3600, 2400, 50, 12)], {
        class: 'lake',
      }),
    ],
  },
];

/** Writes one fixture and reports its byte count and hash. */
function writeFixture(absolutePath, bytes) {
  mkdirSync(path.dirname(absolutePath), { recursive: true });
  writeFileSync(absolutePath, bytes);
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  console.log(`${path.relative(REPOSITORY_ROOT, absolutePath)}: ${bytes.length} bytes ${sha256}`);
}

const z6TileBytes = encodeTile(SOURDOUGH_Z6_LAYERS);
const z9TileBytes = encodeTile(SOURDOUGH_Z9_LAYERS);

writeFixture(
  path.join(CANONICAL_FIXTURE_DIRECTORY, 'sourdough_tile_z6_x18_y27.mvt'),
  z6TileBytes,
);
writeFixture(
  path.join(CANONICAL_FIXTURE_DIRECTORY, 'osmus_tile_z9_x83_y196.mvt'),
  z9TileBytes,
);
// The performance harness serves one file per zoom; both are the synthetic z9
// tile's bytes, exactly as the previous fixtures were copies of one tile.
writeFixture(path.join(PERFORMANCE_FIXTURE_DIRECTORY, 'z7.mvt'), z9TileBytes);
writeFixture(path.join(PERFORMANCE_FIXTURE_DIRECTORY, 'z8.mvt'), z9TileBytes);
