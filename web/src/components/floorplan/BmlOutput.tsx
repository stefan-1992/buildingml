import { useCallback, useState } from 'react';
import { useFloorplanStore } from '@state/floorplan-store.ts';

export function BmlOutput() {
  const bmlOutput = useFloorplanStore((s) => s.bmlOutput);
  const hasUnsavedChanges = useFloorplanStore((s) => s.hasUnsavedChanges);
  const [saved, setSaved] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(bmlOutput);
  };

  const handleSave = useCallback(async () => {
    await useFloorplanStore.getState().saveBml();
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div style={{
        padding: '8px 12px', fontSize: 11, color: 'var(--text-dim)',
        borderBottom: '1px solid var(--border)',
        fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5,
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
      }}>
        <span>
          BML Output
          {hasUnsavedChanges && (
            <span style={{ color: '#f5a623', fontWeight: 400, marginLeft: 6, fontSize: 10 }}>unsaved</span>
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
      <textarea
        readOnly
        value={bmlOutput}
        style={{
          flex: 1, minHeight: 200,
          background: 'var(--bg)', color: 'var(--text)', border: 'none',
          padding: '8px 12px', fontFamily: 'inherit', fontSize: 11,
          lineHeight: 1.5, resize: 'none', outline: 'none', tabSize: 2,
        }}
      />
    </div>
  );
}
