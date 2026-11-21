#!/usr/bin/env node
// Generates the curated bridge's ordinal layer from the frozen UDL and
// round-trips it against the generated ubrn TypeScript declarations.
//
// The curated `MapsSdk` proxy speaks lower-camel enums across the JSON bridge.
// Their ordinal order is contract data: it must come from the UDL, never from
// a hand-written switch that can drift from the generated bindings. This
// script emits the ordinal arrays the proxy uses, then compares every UDL
// enum with the ubrn-generated TypeScript enum of the same name: every variant
// and the order must match, and no enum may exist on only one side.
//
// Usage:
//   node platform/rn/scripts/generate-contract-ordinals.mjs \
//     --udl <frozen-udl-path> --project-root <platform/rn>
//
// The output is `src/generated/rustic_maps-ordinals.ts`. The generation step
// runs this after ubrn, so the drift check covers the file byte-for-byte; a
// contract change that does not regenerate it fails `--check`.

import fs from "node:fs";
import path from "node:path";

const OUTPUT_RELATIVE_PATH = "src/generated/rustic_maps-ordinals.ts";

/** The UDL enum behind each curated bridge enum, and the emitted names. */
const CURATED_ORDINAL_ENUMS = [
  {
    udlName: "RegionDownloadPhase",
    constantName: "regionDownloadPhasesByOrdinal",
    typeName: "RegionDownloadPhase",
  },
  {
    udlName: "PositionSourceError",
    constantName: "positionSourceFailureReasonsByOrdinal",
    typeName: "PositionSourceFailureReason",
  },
];

function parseArguments(argv) {
  const parsedArguments = { udlPath: null, projectRoot: null };
  for (let index = 0; index < argv.length; index += 1) {
    switch (argv[index]) {
      case "--udl":
        parsedArguments.udlPath = argv[index + 1];
        index += 1;
        break;
      case "--project-root":
        parsedArguments.projectRoot = argv[index + 1];
        index += 1;
        break;
      default:
        throw new Error(`unknown argument: ${argv[index]}`);
    }
  }
  if (!parsedArguments.udlPath || !parsedArguments.projectRoot) {
    throw new Error("usage: --udl <path> --project-root <directory>");
  }
  return parsedArguments;
}

/** Removes line comments so a quoted phrase inside a doc comment is not read
 * as an enum variant. */
function stripLineComments(source) {
  return source
    .split("\n")
    .map((line) => {
      const commentIndex = line.indexOf("//");
      return commentIndex === -1 ? line : line.slice(0, commentIndex);
    })
    .join("\n");
}

function parseUdlEnums(udlSource) {
  const source = stripLineComments(udlSource);
  const enums = new Map();
  const enumPattern = /^enum\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{([\s\S]*?)\}/gm;
  for (const match of source.matchAll(enumPattern)) {
    const variants = [];
    for (const line of match[2].split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) {
        continue;
      }
      const quoted = trimmed.match(/^"([^"]+)"\s*,?$/);
      const bare = trimmed.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*,?$/);
      if (quoted) {
        variants.push(quoted[1]);
      } else if (bare) {
        variants.push(bare[1]);
      }
    }
    enums.set(match[1], variants);
  }
  return enums;
}

function parseGeneratedEnums(typescriptSource) {
  const source = typescriptSource
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");
  const enums = new Map();
  const enumPattern =
    /^export enum\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{([\s\S]*?)\}/gm;
  for (const match of source.matchAll(enumPattern)) {
    const variants = [];
    for (const line of match[2].split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) {
        continue;
      }
      const member = trimmed.match(
        /^([A-Za-z_][A-Za-z0-9_]*)\s*(?:=\s*[^,]+)?,?$/,
      );
      if (member) {
        variants.push(member[1]);
      }
    }
    enums.set(match[1], variants);
  }
  return enums;
}

function roundTripEnums(udlEnums, generatedEnums) {
  const problems = [];
  for (const [name, udlVariants] of udlEnums) {
    const generatedVariants = generatedEnums.get(name);
    if (!generatedVariants) {
      problems.push(`enum ${name} is in the UDL but not in the generated TypeScript`);
      continue;
    }
    if (
      udlVariants.length !== generatedVariants.length ||
      udlVariants.some((variant, index) => variant !== generatedVariants[index])
    ) {
      problems.push(
        `enum ${name} differs: UDL ${JSON.stringify(udlVariants)} but generated ${JSON.stringify(generatedVariants)}`,
      );
    }
  }
  // The generated TypeScript also carries ubrn's own helper enums (for
  // example the `_Tags` enums of generated error types); only the contract
  // enums the UDL declares are compared.
  return problems;
}

function lowerCamelCase(variant) {
  return variant.charAt(0).toLowerCase() + variant.slice(1);
}

function renderOrdinalsModule(udlEnums) {
  const lines = [
    "// Generated from the frozen UDL by the React Native generation step.",
    "// Do not edit: the generation round-trips every enum against the",
    "// generated bindings, so the curated bridge cannot invent an ordinal.",
    "",
  ];
  for (const curated of CURATED_ORDINAL_ENUMS) {
    const udlVariants = udlEnums.get(curated.udlName);
    if (!udlVariants) {
      throw new Error(`the UDL does not declare enum ${curated.udlName}`);
    }
    const curatedVariants = udlVariants.map(lowerCamelCase);
    lines.push(`export const ${curated.constantName} = [`);
    for (const variant of curatedVariants) {
      lines.push(`  ${JSON.stringify(variant)},`);
    }
    lines.push("] as const;");
    lines.push(
      `export type ${curated.typeName} = (typeof ${curated.constantName})[number];`,
      "",
    );
  }
  return `${lines.join("\n")}`;
}

function main() {
  const { udlPath, projectRoot } = parseArguments(process.argv.slice(2));
  const udlEnums = parseUdlEnums(fs.readFileSync(udlPath, "utf8"));
  if (udlEnums.size === 0) {
    throw new Error(`no enums found in the UDL: ${udlPath}`);
  }
  const generatedTypescriptPath = path.join(
    projectRoot,
    "src/generated/rustic_maps.ts",
  );
  if (!fs.existsSync(generatedTypescriptPath)) {
    throw new Error(
      `the generated TypeScript bindings are missing: ${generatedTypescriptPath}`,
    );
  }
  const generatedEnums = parseGeneratedEnums(
    fs.readFileSync(generatedTypescriptPath, "utf8"),
  );
  const problems = roundTripEnums(udlEnums, generatedEnums);
  if (problems.length > 0) {
    for (const problem of problems) {
      console.error(`generate-rn-contract-ordinals: ${problem}`);
    }
    process.exit(1);
  }
  const outputPath = path.join(projectRoot, OUTPUT_RELATIVE_PATH);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, renderOrdinalsModule(udlEnums));
  console.log(
    `==> generated the curated ordinal layer for ${CURATED_ORDINAL_ENUMS.length} enums from ${udlEnums.size} UDL enums`,
  );
}

try {
  main();
} catch (error) {
  console.error(`generate-rn-contract-ordinals: ${error.message}`);
  process.exit(1);
}
