import { useAppStore } from '@state/app-store.ts';

export function DiagnosticsPanel() {
  const diagnostics = useAppStore((s) => s.diagnostics);
  const building = useAppStore((s) => s.building);

  return (
    <div style={{
      background: 'var(--surface)',
      display: 'flex', flexDirection: 'column',
      overflow: 'hidden',
    }}>
      <div style={{
        padding: '6px 12px', fontSize: 11, color: 'var(--text-dim)',
        borderBottom: '1px solid var(--border)',
      }}>
        Diagnostics
      </div>
      <div style={{
        padding: '8px 12px', fontSize: 12,
        flex: 1, overflowY: 'auto',
      }}>
        {building && <span style={{ color: 'var(--text-dim)' }}>Building...</span>}
        {!building && diagnostics.length === 0 && (
          <span style={{ color: 'var(--success)' }}>No issues found.</span>
        )}
        {!building && diagnostics.map((d, i) => {
          const cls = d.severity === 'error' ? 'var(--error)' :
                      d.severity === 'warning' ? 'var(--warning)' : 'var(--accent)';
          const loc = d.line ? `L${d.line}:${d.column || 0}` : '';
          return (
            <div key={i} style={{ padding: '2px 0', color: cls }}>
              [{d.code}] {loc} {d.message}
            </div>
          );
        })}
      </div>
    </div>
  );
}
