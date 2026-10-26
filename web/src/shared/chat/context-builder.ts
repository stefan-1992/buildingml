import type { SemanticModel, ResolvedElement, ResolvedProperty, Diagnostic } from '../types.ts';
import { getStoreys, planElements } from '../floorplan/model-importer.ts';

/**
 * Build a context string for the LLM from the current editor state.
 * Sends only the relevant BML excerpt (current storey, selected element)
 * rather than the entire project, keeping token usage efficient.
 */
export function buildChatContext(opts: {
  source: string;
  model: SemanticModel | null;
  currentStoreyIndex: number;
  selectedElementId: string | null;
  diagnostics: Diagnostic[];
  viewMode: '3d' | '2d' | 'split' | 'ifc';
}): string {
  const parts: string[] = [];

  parts.push('You are a BuildingML assistant. BuildingML (BML) is a domain-specific language for describing buildings.');
  parts.push('');
  parts.push('CRITICAL: When modifying an EXISTING building, you MUST use SEARCH/REPLACE blocks. NEVER output the full file.');
  parts.push('When generating a NEW building (no existing source shown), output the full BML inside a ```bml code block instead.');
  parts.push('DO NOT wrap your changes in ```bml code blocks. Use ONLY this format:');
  parts.push('');
  parts.push('<<<<<<< SEARCH');
  parts.push('exact lines copied from the current source');
  parts.push('=======');
  parts.push('replacement lines');
  parts.push('>>>>>>> REPLACE');
  parts.push('');
  parts.push('Example — user says "make wall W-01 thicker":');
  parts.push('');
  parts.push('<<<<<<< SEARCH');
  parts.push('  Wall: W-01');
  parts.push('    from: (0, 0) to: (4, 0)');
  parts.push('    thickness: 0.24 m');
  parts.push('=======');
  parts.push('  Wall: W-01');
  parts.push('    from: (0, 0) to: (4, 0)');
  parts.push('    thickness: 0.36 m');
  parts.push('>>>>>>> REPLACE');
  parts.push('');
  parts.push('Rules:');
  parts.push('- SEARCH must match the current source EXACTLY (whitespace, indentation, everything).');
  parts.push('- Include enough surrounding lines to make the match unique.');
  parts.push('- Use multiple blocks for multiple changes.');
  parts.push('- To add new elements, include a nearby line in SEARCH as an anchor.');
  parts.push('- If the user asks a question without requesting changes, answer normally (no blocks).');
  parts.push('');

  // BML syntax reference
  parts.push('BML syntax reference:');
  parts.push('CRITICAL: Heading lines (# ## ### ####) MUST start at column 0 — NO leading spaces. Only property lines are indented.');
  parts.push('- # Site: <name>  (top level, column 0)');
  parts.push('- ## Building: <name>  (column 0)');
  parts.push('- ### Storey: <name>  (column 0, with indented elevation: and height: below)');
  parts.push('- #### Space: <name>  (column 0, with indented usage: below)');
  parts.push('- Element types: Wall, Slab, Column, Beam, Door, Window, Roof, Stair, Railing, Ramp, Furniture, Elevator');
  parts.push('- Wall: <id>  (indented 2 spaces) with from: (x1, y1) to: (x2, y2), height:, thickness:');
  parts.push('- Door/Window (indented under parent Wall) with offset:, width:, height:, sill_height:');
  parts.push('- Slab: <id>  with boundary: [(x1,y1), (x2,y2), ...], thickness:');
  parts.push('- Roof: <id>  (indented element inside a Space, NOT a heading) with boundary: [...], type: flat|gable|hip|shed, ridge_height: (REQUIRED for all types — use 0 m for flat roofs)');
  parts.push('- Column: <id>  with position: (x, y), height:, profile: rectangle(w, d)');
  parts.push('- Beam: <id>  with from: (x1, y1) to: (x2, y2), profile: rectangle(w, h)');
  parts.push('- Properties are indented key: value pairs');
  parts.push('- Units: m (meters), m2 (square meters)');
  parts.push('- material: (optional) applies to Wall, Slab, Roof, Column, Beam, Railing, Ramp — valid values: concrete, wood, timber, brick, glass, steel, stone, gypsum, insulation');
  parts.push('  IMPORTANT: when the user asks for the whole building to be made of one material, add material: to EVERY structural element — Wall, Slab, Roof, Column, Beam, etc. Do NOT skip any element type.');
  parts.push('- Space usage valid values: living, bedroom, kitchen, bathroom, office, corridor, staircase, storage, garage, utility, lobby, dining, balcony, parking, mechanical');
  parts.push('');
  parts.push('ROOF PLACEMENT RULE (critical for correct height):');
  parts.push('The roof 3D position is calculated as: elevation + storey_height.');
  parts.push('ALWAYS place a Roof in a DEDICATED top-most storey (e.g. RoofLevel) that contains NO walls — only a single Space with the Roof element.');
  parts.push('Set the RoofLevel elevation to the top of the last living floor (sum of all living storey heights).');
  parts.push('Set RoofLevel height to the desired eave height above the walls (typically 0–1 m for a low eave, 3 m for a tall attic).');
  parts.push('The roof boundary must cover the ENTIRE building footprint, not just one room.');
  parts.push('NEVER place a Roof inside a Space that also has walls — this causes the roof to appear at the wrong height.');
  parts.push('Example — pitched roof on a 2-storey building (walls end at 6 m):');
  parts.push('### Storey: RoofLevel');
  parts.push('  elevation: 6 m');
  parts.push('  height: 0 m');
  parts.push('#### Space: AtticSpace');
  parts.push('  usage: storage');
  parts.push('  Roof: RF-01');
  parts.push('    boundary: [(0, 0), (10, 0), (10, 8), (0, 8)]');
  parts.push('    type: gable');
  parts.push('    ridge_height: 2.5 m');
  parts.push('    material: wood');
  parts.push('Example — flat roof (ridge_height must be 0 m):');
  parts.push('### Storey: RoofLevel');
  parts.push('  elevation: 6 m');
  parts.push('  height: 0 m');
  parts.push('#### Space: RoofDeck');
  parts.push('  usage: storage');
  parts.push('  Roof: RF-01');
  parts.push('    boundary: [(0, 0), (10, 0), (10, 8), (0, 8)]');
  parts.push('    type: flat');
  parts.push('    ridge_height: 0 m');
  parts.push('    material: concrete');
  parts.push('');
  parts.push('Correct structure example (note #### at column 0):');
  parts.push('### Storey: GroundFloor');
  parts.push('  elevation: 0 m');
  parts.push('  height: 3 m');
  parts.push('#### Space: LivingRoom');
  parts.push('  usage: living');
  parts.push('  Wall: W-01');
  parts.push('    from: (0, 0) to: (6, 0)');
  parts.push('    height: 3 m');
  parts.push('    thickness: 0.24 m');
  parts.push('    Door: D-01');
  parts.push('      offset: 1.0 m');
  parts.push('      width: 0.9 m');
  parts.push('      height: 2.1 m');
  parts.push('');

  // Multi-room design reasoning guidance
  parts.push('MULTI-ROOM DESIGN — follow these steps before writing BML:');
  parts.push('1. List every room and its approximate position/size.');
  parts.push('2. For each pair of adjacent rooms, identify the wall they share.');
  parts.push('3. Decide which shared wall gets a door, and place the Door element there.');
  parts.push('4. Verify: can every room be reached from every other room through doors?');
  parts.push('');
  parts.push('BML DOOR RULE: A Door inside a Wall connects that Wall\'s parent Space to the space on the OTHER SIDE of that wall.');
  parts.push('A door on an exterior perimeter wall only creates an entrance/exit to the outside — it does NOT connect two interior spaces.');
  parts.push('Every walled interior Space must have at least one Door on a wall shared with an adjacent space.');
  parts.push('');
  parts.push('RESIDENTIAL LAYOUT (no corridor): rooms connect directly to each other.');
  parts.push('- Living room is the central hub: bedrooms and kitchen open directly to it via doors on shared walls.');
  parts.push('- Bathrooms open to an adjacent bedroom (en-suite) or to the living area via a shared wall door.');
  parts.push('- Kitchen is adjacent to the living/dining space and connects through a shared wall or open archway (Door).');
  parts.push('');

  // Current state
  parts.push('--- Current BML source ---');
  parts.push(opts.source);
  parts.push('--- End of BML source ---');
  parts.push('');

  // View context
  parts.push(`The user is currently in ${opts.viewMode.toUpperCase()} view mode.`);

  // Storey context
  if (opts.model) {
    const storey = getStoreys(opts.model)[opts.currentStoreyIndex];
    if (storey) {
      parts.push(`Active ${storey.level}: "${storey.name}"`);
      if (storey.spaces.length > 0) {
        parts.push(`Spaces in this ${storey.level}: ${storey.spaces.map(s => s.name).join(', ')}`);
      }
    }
  }

  // Selected element context
  if (opts.selectedElementId && opts.model) {
    const elem = findElement(opts.model, opts.selectedElementId);
    if (elem) {
      parts.push(`Currently selected: ${elem.element_type} "${elem.id}" with properties: ${formatProperties(elem.properties)}`);
    }
  }

  // Diagnostics
  const errors = opts.diagnostics.filter(d => d.severity === 'error');
  if (errors.length > 0) {
    parts.push(`Current errors (${errors.length}): ${errors.slice(0, 3).map(e => e.message).join('; ')}`);
  }
  const warnings = opts.diagnostics.filter(d => d.severity === 'warning');
  if (warnings.length > 0) {
    parts.push(`Current warnings (${warnings.length}): ${warnings.slice(0, 5).map(w => w.message).join('; ')}`);
  }

  return parts.join('\n');
}

function findElement(model: SemanticModel, id: string): ResolvedElement | null {
  for (const plan of getStoreys(model)) {
    for (const elem of planElements(plan)) {
      if (elem.id === id) return elem;
      for (const child of elem.children) {
        if (child.id === id) return child;
      }
    }
  }
  return null;
}

function formatProperties(props: ResolvedProperty[]): string {
  return props.map(p => {
    const v = p.value;
    if ('Number' in v) return `${p.key}: ${v.Number}`;
    if ('String' in v) return `${p.key}: "${v.String}"`;
    if ('Boolean' in v) return `${p.key}: ${v.Boolean}`;
    if ('Identifier' in v) return `${p.key}: ${v.Identifier}`;
    if ('Axis' in v) return `${p.key}: (${v.Axis.from.join(',')}) to (${v.Axis.to.join(',')})`;
    if ('Point' in v) return `${p.key}: (${v.Point.join(',')})`;
    return `${p.key}: [complex]`;
  }).join(', ');
}

export interface SearchReplaceBlock {
  search: string;
  replace: string;
}

/**
 * Extract SEARCH/REPLACE blocks from the LLM response.
 * Returns an array of {search, replace} pairs.
 */
export function extractSearchReplaceBlocks(response: string): SearchReplaceBlock[] {
  const blocks: SearchReplaceBlock[] = [];
  // Allow empty REPLACE sections (no \n required before >>>>>>> REPLACE)
  const regex = /<<<<<<< SEARCH\n([\s\S]*?)\n?=======\n([\s\S]*?)>>>>>>> REPLACE/g;
  let match;
  while ((match = regex.exec(response)) !== null) {
    // Trim trailing newline from replace content (artifact of the regex)
    const replace = match[2].endsWith('\n') ? match[2].slice(0, -1) : match[2];
    blocks.push({ search: match[1], replace });
  }
  return blocks;
}

/**
 * Apply SEARCH/REPLACE blocks to source text.
 * Returns the modified source, or null if any search block wasn't found.
 */
export function applySearchReplaceBlocks(source: string, blocks: SearchReplaceBlock[]): { result: string; errors: string[] } {
  let result = source;
  const errors: string[] = [];

  for (const block of blocks) {
    const idx = result.indexOf(block.search);
    if (idx === -1) {
      errors.push(`Could not find text to replace: "${block.search.slice(0, 60)}..."`);
      continue;
    }
    result = result.slice(0, idx) + block.replace + result.slice(idx + block.search.length);
  }

  return { result, errors };
}

/**
 * Extract a BML code block from the LLM response (full file replacement, fallback).
 * Returns the BML source if found, null otherwise.
 */
export function extractBmlFromResponse(response: string): string | null {
  // Look for ```bml ... ``` code blocks
  const bmlMatch = response.match(/```bml\n([\s\S]*?)```/);
  if (bmlMatch) return bmlMatch[1].trim();

  // Also try generic code blocks that look like BML (start with # Site:)
  const genericMatch = response.match(/```\n(#\s+Site:[\s\S]*?)```/);
  if (genericMatch) return genericMatch[1].trim();

  return null;
}
