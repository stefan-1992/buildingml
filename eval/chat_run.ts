// ============================================================
// Edit PROMPT_INDEX (1–7) to select which prompt to run, then:
//   node --experimental-strip-types eval/chat_run.ts
// ============================================================

const PROMPT_INDEX = 10;  // <-- change this (1–7)

const PROMPTS: Record<number, string> = {
  1: `I want to build a two-story hotel with eight rooms on each floor. The rooms are arranged in groups of four on each side, 
  separated by a 4-meter-wide corridor in the middle. Each room has a door and a window. 
  The doors of the rooms are on the corridor side of the wall, and the windows are on the outside wall of the building. 
  The building should have a wooden pitched roof and brick walls.`,

  2: `Create a basic 3D model of a four-story residential house with dimensions of 5 by 3 meters.`,

  3: `Create a single-story residential house with a total floor area of 120 square meters. 
  Includes three bedrooms, two bathrooms, a kitchen, and a living room. 
  The house should have a wooden pitched roof, and incorporate at least four windows and one main entrance door. Use the concrete walls.`,

  4: `Create a building with three connected sections. Each section should be a rectangle (10m x 5m) with walls of 3 meters in height. 
  The sections should be connected by 5-meter-long walls. Add slabs and a continuous, concrete-pitched roof that covers all sections. 
  Add doors and windows to each section. Choose the right material for the wall.`,

  5: `Create a 3-story L-shaped house with each leg of the L being 8 meters long and 4 meters wide. 
  Place a door at the corner of the L and a window on each side of the L. I want the whole building to be made of wood.`,

  6: `Design a building with a complex polygonal footprint (e.g., hexagonal). Each side of the hexagon should be 5 meters. 
  Add a slab for the floor and a pitched roof. Include a door on one side and a window on each of the other sides.`,

  7: `Construct a residential building with a rectangular footprint (15m x 10m), a pitched roof and two floors. 
  Create balconies by extending the floor slab outwards from the exterior walls on the first floor. Add doors and windows to each floor. 
  Make sure that the balconies are accessible from the inside.`,

  8: `Construct a modern office building with a rectangular base of 20 meters by 20 meters. Set the wall height to 3 meters. Include four
rooms (5x5 meters each) along the perimeter, with a central open space. Add doors and windows to each room and a main entrance
door to the building.`,

  9: `Design a two-story apartment building with an H-shaped base. Each floor consists of two apartments with two rooms each.`,

  10: `Create a T-shaped, single-story building with a horizontal section of 10 meters x 30 meters and a vertical section of 10 meters x 20
meters. Connect the two sections by placing a door at their junction. Each section has three windows. The entire building is made of
concrete.`,
};

const prompt = PROMPTS[PROMPT_INDEX];
if (!prompt) { console.error(`Invalid PROMPT_INDEX: ${PROMPT_INDEX}. Must be 1–10.`); process.exit(1); }

const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
const outFile = `eval/output_p${PROMPT_INDEX}_${ts}.bml`;
const source  = "";                      // path to existing .bml file to modify (leave empty to generate new)
const model   = "claude-sonnet-4-6";    // or "claude-opus-4-8"

// ============================================================

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { buildChatContext, extractSearchReplaceBlocks, applySearchReplaceBlocks, extractBmlFromResponse } from "../web/src/shared/chat/context-builder.ts";
import type { SemanticModel, Diagnostic } from "../web/src/shared/types.ts";

const sourceContent = source ? readFileSync(source, "utf8") : "";

// Get semantic model + diagnostics if editing an existing file
let semanticModel: SemanticModel | null = null;
let diagnostics: Diagnostic[] = [];
if (sourceContent) {
  const tmp = join(tmpdir(), `chat_run_${Date.now()}.bml`);
  writeFileSync(tmp, sourceContent);
  const mr = spawnSync("buildingml", ["model", tmp], { encoding: "utf8" });
  if (mr.status === 0) semanticModel = JSON.parse(mr.stdout) as SemanticModel;
  const lr = spawnSync("buildingml", ["lint", "--format", "json", tmp], { encoding: "utf8" });
  if (lr.stdout) try { diagnostics = JSON.parse(lr.stdout).diagnostics ?? []; } catch {}
  unlinkSync(tmp);
}

// Build prompt using the real context-builder
const systemPrompt = buildChatContext({
  source: sourceContent,
  model: semanticModel,
  currentStoreyIndex: 0,
  selectedElementId: null,
  diagnostics,
  viewMode: "3d",
});

const fullPrompt = `${systemPrompt}\n\nUser: ${prompt.trim()}`;
console.log(`model=${model}  prompt=${fullPrompt.length} chars\n`);

// Call LLM
const result = spawnSync("claude", ["--print", "--model", model], {
  input: fullPrompt, encoding: "utf8", maxBuffer: 10 * 1024 * 1024,
});
if (result.status !== 0 || !result.stdout) {
  console.error("claude error:", result.stderr?.slice(0, 400));
  process.exit(1);
}

const response = result.stdout.trim();
console.log(response);

// Extract BML
const blocks = extractSearchReplaceBlocks(response);
const bmlBlock = extractBmlFromResponse(response);
let finalBml: string | null = null;

if (blocks.length > 0) {
  const { result: updated, errors } = applySearchReplaceBlocks(sourceContent, blocks);
  if (errors.length) { console.error("SEARCH/REPLACE errors:", errors); process.exit(1); }
  finalBml = updated;
} else if (bmlBlock) {
  finalBml = bmlBlock;
}

if (!finalBml) { console.log("\n(no BML in response)"); process.exit(0); }

// Lint
const tmp2 = join(tmpdir(), `chat_run_out_${Date.now()}.bml`);
writeFileSync(tmp2, finalBml);
const lint = spawnSync("buildingml", ["lint", tmp2], { encoding: "utf8" });
unlinkSync(tmp2);
console.log(`\nlint: ${lint.status === 0 ? "OK" : "ERRORS"}`);
console.log((lint.stdout + lint.stderr).trim());

if (lint.status !== 0) process.exit(1);

// Save
mkdirSync(dirname(outFile), { recursive: true });
writeFileSync(outFile, finalBml, "utf8");
console.log(`\nsaved → ${outFile}`);
console.log(`preview: buildingml preview ${outFile}`);
