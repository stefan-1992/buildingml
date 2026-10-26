/**
 * Dark theme for the BML CodeMirror editor.
 * Matches the app's dark UI palette.
 */

import { EditorView } from '@codemirror/view';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';

/** Editor chrome theme (gutters, selection, cursor, etc). */
export const bmlEditorTheme = EditorView.theme({
  '&': {
    backgroundColor: 'var(--bg, #181825)',
    color: 'var(--text, #cdd6f4)',
    fontSize: '11px',
    fontFamily: 'inherit',
    height: 'auto',
  },
  '.cm-content': {
    caretColor: 'var(--accent, #89b4fa)',
    lineHeight: '1.5',
    padding: '8px 0',
  },
  '.cm-cursor, .cm-dropCursor': {
    borderLeftColor: 'var(--accent, #89b4fa)',
  },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
    backgroundColor: 'rgba(137, 180, 250, 0.2)',
  },
  '.cm-gutters': {
    backgroundColor: 'var(--bg, #181825)',
    color: 'rgba(205, 214, 244, 0.3)',
    border: 'none',
    fontSize: '10px',
    minWidth: '32px',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'rgba(137, 180, 250, 0.08)',
    color: 'rgba(205, 214, 244, 0.6)',
  },
  '.cm-activeLine': {
    backgroundColor: 'rgba(137, 180, 250, 0.04)',
  },
  '.cm-matchingBracket': {
    backgroundColor: 'rgba(137, 180, 250, 0.15)',
    outline: '1px solid rgba(137, 180, 250, 0.3)',
  },
  '.cm-foldPlaceholder': {
    backgroundColor: 'rgba(137, 180, 250, 0.1)',
    border: '1px solid rgba(137, 180, 250, 0.2)',
    color: 'var(--text-dim, #a6adc8)',
  },
  '.cm-tooltip': {
    backgroundColor: 'var(--surface, #1e1e2e)',
    border: '1px solid var(--border, rgba(255,255,255,0.08))',
    color: 'var(--text, #cdd6f4)',
  },
  '.cm-scroller': {
    overflow: 'auto',
  },
  // Error line decoration
  '.cm-bml-error-line': {
    backgroundColor: 'rgba(243, 139, 168, 0.06)',
    borderLeft: '2px solid var(--error, #f38ba8)',
  },
}, { dark: true });

/** Syntax highlighting colours for BML tokens. */
export const bmlSyntaxHighlighting = syntaxHighlighting(HighlightStyle.define([
  { tag: t.heading, color: '#89b4fa', fontWeight: 'bold' },
  { tag: t.className, color: '#cba6f7', fontWeight: 'bold' },
  { tag: t.propertyName, color: '#94e2d5' },
  { tag: t.keyword, color: '#f38ba8' },
  { tag: t.number, color: '#fab387' },
  { tag: t.string, color: '#a6e3a1' },
  { tag: t.lineComment, color: '#6c7086', fontStyle: 'italic' },
  { tag: t.docComment, color: '#74c7ec', fontStyle: 'italic' },
  { tag: t.variableName, color: '#f9e2af' },
  { tag: t.paren, color: '#9399b2' },
]));
