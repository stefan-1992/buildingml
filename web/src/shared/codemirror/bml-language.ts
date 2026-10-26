/**
 * BuildingML (BML) language support for CodeMirror 6.
 *
 * Uses StreamLanguage with a simple tokenizer for syntax highlighting.
 */

import { StreamLanguage, type StreamParser } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';

/** Known BML element types for highlighting. */
const ELEMENT_TYPES = new Set([
  'Wall', 'Slab', 'Column', 'Beam', 'Roof', 'Staircase', 'Stair',
  'Railing', 'Ramp', 'Door', 'Window', 'Furniture', 'Elevator',
  'Proxy', 'Member', 'Plate', 'Opening', 'Grid', 'Covering', 'Footing',
  'Duct', 'Pipe', 'CableTray', 'Terminal', 'Equipment', 'Accessory', 'System',
  'CurtainWall', 'Space', 'Template', 'CheckSuite', 'Check',
  'AssetLibrary', 'Asset', 'Part',
]);


interface BmlParserState {
  inBlockComment: boolean;
}

const bmlStreamParser: StreamParser<BmlParserState> = {
  name: 'bml',
  startState(): BmlParserState {
    return { inBlockComment: false };
  },
  token(stream, _state): string | null {
    // Skip whitespace
    if (stream.eatSpace()) return null;

    // Doc comments (///)
    if (stream.match('///')) {
      stream.skipToEnd();
      return 'docComment';
    }

    // Line comments (//)
    if (stream.match('//')) {
      stream.skipToEnd();
      return 'lineComment';
    }

    // Headings (# to ####)
    if (stream.sol() && stream.peek() === '#') {
      const match = stream.match(/^#{1,5}\s+/);
      if (match) {
        // Check for element type declaration: "### Type: ID" or "### Type:"
        const rest = stream.string.slice(stream.pos);
        const typeMatch = rest.match(/^(\w+):\s*([\w-]*)/);
        if (typeMatch && ELEMENT_TYPES.has(typeMatch[1])) {
          stream.skipToEnd();
          return 'heading';
        }
        stream.skipToEnd();
        return 'heading';
      }
    }

    // Element type declarations at start of line: "Type: ID" or "Type:"
    if (stream.sol()) {
      const saved = stream.pos;
      const wordMatch = stream.match(/^(\w+):\s*/);
      if (wordMatch) {
        const word = (wordMatch as RegExpMatchArray)[1];
        if (ELEMENT_TYPES.has(word)) {
          // Rest of line is the element ID
          stream.skipToEnd();
          return 'typeName';
        }
        // Could be a property key at indented level — rewind
        stream.pos = saved;
      }
    }

    // Property key-value: "  key:" at start of line (with indentation)
    if (stream.sol()) {
      const saved = stream.pos;
      const indentedProp = stream.match(/^(\s+)(\w[\w_]*):\s*/);
      if (indentedProp) {
        // The rest is the value — don't consume it, just mark the key
        // Actually stream already consumed the key, return propertyName
        return 'propertyName';
      }
      stream.pos = saved;
    }

    // import keyword
    if (stream.sol() && stream.match(/^import\s+/)) {
      stream.skipToEnd();
      return 'keyword';
    }

    // Curve keywords in point lists: smooth (x, y), nurbs { ... }
    if (stream.match(/^(smooth|nurbs)\b/)) {
      return 'keyword';
    }

    // Numbers (with optional units)
    if (stream.match(/^-?\d+(\.\d+)?\s*(mm|cm|m|deg|°)?/)) {
      return 'number';
    }

    // Coordinate tuples: (x, y) or (x, y, z)
    if (stream.match(/^\(/)) {
      return 'paren';
    }
    if (stream.match(/^\)/)) {
      return 'paren';
    }

    // Strings (quoted)
    if (stream.peek() === '"') {
      stream.next();
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === '"') break;
        if (ch === '\\') stream.next();
      }
      return 'string';
    }

    // Variable substitution {var}
    if (stream.match(/^\{[\w]+\}/)) {
      return 'variableName';
    }

    // Default: consume one character
    stream.next();
    return null;
  },
  languageData: {
    commentTokens: { line: '//' },
  },
};

/** Tag-based styling map for the BML tokenizer. */
export const bmlHighlighting = {
  heading: t.heading,
  typeName: t.className,
  propertyName: t.propertyName,
  keyword: t.keyword,
  number: t.number,
  string: t.string,
  lineComment: t.lineComment,
  docComment: t.docComment,
  variableName: t.variableName,
  paren: t.paren,
};

/** CodeMirror Language instance for BuildingML. */
export const bmlLanguage = StreamLanguage.define(bmlStreamParser);
