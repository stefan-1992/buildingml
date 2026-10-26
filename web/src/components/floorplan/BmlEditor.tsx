import { useCallback, useRef, useState, useEffect } from 'react';
import { useFloorplanStore } from '@state/floorplan-store.ts';
import { useAppStore } from '@state/app-store.ts';
import { BmlCodeMirror } from '@shared/codemirror/BmlCodeMirror.tsx';
import type { Diagnostic } from '@shared/types.ts';

export function BmlEditor() {
  const bmlOutput = useFloorplanStore((s) => s.bmlOutput);
  const hasUnsavedChanges = useFloorplanStore((s) => s.hasUnsavedChanges);
  const provider = useAppStore((s) => s.provider);

  const [localValue, setLocalValue] = useState(bmlOutput);
  const [isEditing, setIsEditing] = useState(false);
  const [parseErrors, setParseErrors] = useState<Diagnostic[]>([]);
  const [saved, setSaved] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const parseTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastExternalBml = useRef(bmlOutput);

  // Track external changes (from 2D editor) when not actively editing
  useEffect(() => {
    if (!isEditing && bmlOutput !== lastExternalBml.current) {
      setLocalValue(bmlOutput);
      lastExternalBml.current = bmlOutput;
    }
  }, [bmlOutput, isEditing]);

  const handleChange = useCallback((value: string) => {
    setLocalValue(value);
    setIsEditing(true);

    // Debounced parse and sync
    if (parseTimeoutRef.current) clearTimeout(parseTimeoutRef.current);
    parseTimeoutRef.current = setTimeout(async () => {
      try {
        setSyncing(true);
        // Lint first to check for errors
        const diags = await provider.lint(value);
        const errors = diags.filter(d => d.severity === 'error');
        setParseErrors(errors);

        if (errors.length === 0) {
          // Parse successful — generate model and update 2D editor
          const model = await provider.generateModel(value);
          useFloorplanStore.getState().applyBmlSource(value, model);
          lastExternalBml.current = value;
        }
      } catch {
        // Parse failed — keep showing the text but don't update 2D
      } finally {
        setSyncing(false);
        setIsEditing(false);
      }
    }, 600);
  }, [provider]);

  const handleSave = useCallback(async () => {
    await useFloorplanStore.getState().saveBml();
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  }, []);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(localValue);
  }, [localValue]);

  const handleFocus = useCallback(() => {
    setIsEditing(true);
  }, []);

  const handleBlur = useCallback(() => {
    // Small delay to allow pending parse to complete
    setTimeout(() => setIsEditing(false), 100);
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden', flex: 1 }}>
      <div style={{
        padding: '8px 12px', fontSize: 11, color: 'var(--text-dim)',
        borderBottom: '1px solid var(--border)',
        fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5,
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
      }}>
        <span>
          BML Source
          {hasUnsavedChanges && (
            <span style={{ color: '#f5a623', fontWeight: 400, marginLeft: 6, fontSize: 10 }}>unsaved</span>
          )}
          {syncing && (
            <span style={{ color: 'var(--accent)', fontWeight: 400, marginLeft: 6, fontSize: 10 }}>syncing...</span>
          )}
          {parseErrors.length > 0 && (
            <span style={{ color: 'var(--error, #f38ba8)', fontWeight: 400, marginLeft: 6, fontSize: 10 }}>
              {parseErrors.length} error{parseErrors.length !== 1 ? 's' : ''}
            </span>
          )}
        </span>
        <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
          {saved && <span style={{ fontSize: 10, color: '#a6e3a1' }}>Saved</span>}
          <button
            onClick={handleSave}
            title="Save (Ctrl+S)"
            style={{
              background: hasUnsavedChanges ? 'rgba(245,166,35,0.15)' : 'transparent',
              border: hasUnsavedChanges ? '1px solid rgba(245,166,35,0.4)' : 'none',
              color: hasUnsavedChanges ? '#f5a623' : 'var(--text)',
              padding: '2px 8px', fontSize: 10, cursor: 'pointer', fontFamily: 'inherit',
              borderRadius: 3,
            }}
          >
            Save
          </button>
          <button
            onClick={handleCopy}
            style={{
              background: 'transparent', border: 'none', color: 'var(--text)',
              padding: '2px 8px', fontSize: 10, cursor: 'pointer', fontFamily: 'inherit',
            }}
          >
            Copy
          </button>
        </div>
      </div>
      {parseErrors.length > 0 && (
        <div style={{
          padding: '4px 12px', fontSize: 10, lineHeight: 1.4,
          background: 'rgba(243,139,168,0.08)', borderBottom: '1px solid var(--border)',
          color: 'var(--error, #f38ba8)', maxHeight: 60, overflowY: 'auto',
        }}>
          {parseErrors.slice(0, 3).map((d, i) => (
            <div key={i}>
              {d.line ? `L${d.line}: ` : ''}{d.message}
            </div>
          ))}
        </div>
      )}
      <BmlCodeMirror
        value={localValue}
        onChange={handleChange}
        onFocus={handleFocus}
        onBlur={handleBlur}
        diagnostics={parseErrors}
      />
    </div>
  );
}
