/**
 * React component wrapping a CodeMirror 6 editor for BuildingML source.
 *
 * Features:
 *   - BML syntax highlighting
 *   - Line numbers and active line
 *   - Error line decorations
 *   - Controlled value via onChange/value props
 *   - Dark theme matching the app palette
 */

import { useRef, useEffect, useCallback } from 'react';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from '@codemirror/view';
import { defaultKeymap, indentWithTab, history, historyKeymap } from '@codemirror/commands';
import { bracketMatching } from '@codemirror/language';
import { bmlLanguage } from './bml-language.ts';
import { bmlEditorTheme, bmlSyntaxHighlighting } from './bml-theme.ts';
import type { Diagnostic } from '@shared/types.ts';

export interface BmlCodeMirrorProps {
  value: string;
  onChange: (value: string) => void;
  onFocus?: () => void;
  onBlur?: () => void;
  /** Diagnostics to display as error line highlights. */
  diagnostics?: Diagnostic[];
  /** Prevent keyboard events from propagating (e.g., to floorplan shortcuts). */
  stopKeyPropagation?: boolean;
  /** Placeholder text when editor is empty. */
  placeholder?: string;
  /** Whether the editor is read-only. */
  readOnly?: boolean;
}

export function BmlCodeMirror({
  value,
  onChange,
  onFocus,
  onBlur,
  diagnostics = [],
  stopKeyPropagation = true,
  readOnly = false,
}: BmlCodeMirrorProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  const suppressUpdateRef = useRef(false);
  const valueRef = useRef(value);

  // Keep refs up to date
  onChangeRef.current = onChange;
  valueRef.current = value;

  // Create editor on mount
  useEffect(() => {
    if (!containerRef.current) return;

    const extensions: Extension[] = [
      lineNumbers(),
      highlightActiveLine(),
      highlightActiveLineGutter(),
      history(),
      bracketMatching(),
      bmlLanguage,
      bmlEditorTheme,
      bmlSyntaxHighlighting,
      keymap.of([
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged && !suppressUpdateRef.current) {
          const newDoc = update.state.doc.toString();
          valueRef.current = newDoc;
          onChangeRef.current(newDoc);
        }
      }),
      EditorState.readOnly.of(readOnly),
      EditorView.lineWrapping,
    ];

    const state = EditorState.create({
      doc: valueRef.current,
      extensions,
    });

    const view = new EditorView({
      state,
      parent: containerRef.current,
    });

    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // Only run on mount — value/readOnly changes handled via effects below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sync external value changes into the editor
  useEffect(() => {
    // Always track the latest value so remount picks it up
    valueRef.current = value;

    const view = viewRef.current;
    if (!view) return;

    const currentDoc = view.state.doc.toString();
    if (currentDoc === value) return;

    suppressUpdateRef.current = true;
    view.dispatch({
      changes: { from: 0, to: currentDoc.length, insert: value },
    });
    suppressUpdateRef.current = false;
  }, [value]);

  // Handle focus/blur
  const handleFocusCapture = useCallback(() => onFocus?.(), [onFocus]);
  const handleBlurCapture = useCallback(() => onBlur?.(), [onBlur]);

  // Stop key propagation when focused (prevents floorplan shortcuts)
  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (stopKeyPropagation) {
      e.stopPropagation();
    }
  }, [stopKeyPropagation]);

  return (
    <div
      ref={containerRef}
      onFocusCapture={handleFocusCapture}
      onBlurCapture={handleBlurCapture}
      onKeyDown={handleKeyDown}
      style={{
        flex: 1,
        minHeight: 0,
        overflow: 'auto',
        contain: 'strict',
        borderLeft: diagnostics.length > 0
          ? '2px solid var(--error, #f38ba8)'
          : '2px solid transparent',
      }}
    />
  );
}
