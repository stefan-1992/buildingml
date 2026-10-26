import { useViewerStore } from '@state/viewer-store.ts';

/**
 * Read-only metadata panel for a selected 3D element.
 * Displays element type, ID, label, and all properties from the SceneNode.
 */
export function Viewer3DPropertiesPanel() {
  const selectedElement = useViewerStore((s) => s.selectedElement);
  const setSelectedElement = useViewerStore((s) => s.setSelectedElement);

  if (!selectedElement) {
    return (
      <div style={{ padding: '8px 12px', color: 'var(--text-dim)', fontSize: 12 }}>
        Click an element in the 3D view to inspect its properties
      </div>
    );
  }

  return (
    <div style={{ overflow: 'auto', maxHeight: '100%' }}>
      <Section title={selectedElement.elementType}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
          <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>
            {selectedElement.label}
          </span>
          <button
            onClick={() => setSelectedElement(null)}
            style={{
              background: 'none', border: 'none', color: 'var(--text-dim)',
              cursor: 'pointer', fontSize: 14, lineHeight: 1, padding: '0 2px',
            }}
            title="Clear selection"
          >
            x
          </button>
        </div>
        <PropRow label="ID" value={selectedElement.id} />
        <PropRow label="Type" value={selectedElement.elementType} />
        {selectedElement.properties.map(([key, val]) => (
          <PropRow key={key} label={formatLabel(key)} value={val} />
        ))}
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border)' }}>
      <div style={{
        fontSize: 10, fontWeight: 600, textTransform: 'uppercase',
        letterSpacing: 0.5, color: 'var(--text-dim)', marginBottom: 6,
      }}>
        {title}
      </div>
      {children}
    </div>
  );
}

function PropRow({ label, value }: { label: string; value: string }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
      <span style={{ fontSize: 11, color: 'var(--text-dim)', minWidth: 80, flexShrink: 0 }}>
        {label}
      </span>
      {/* IFC material and style names are often far too long for the panel,
          so the full value is available on hover. */}
      <span title={value} style={{
        fontSize: 11, color: 'var(--text)',
        fontFamily: 'inherit', overflow: 'hidden', textOverflow: 'ellipsis',
        whiteSpace: 'nowrap', flex: 1,
      }}>
        {value}
      </span>
    </div>
  );
}

/** Convert snake_case property keys to Title Case labels. */
function formatLabel(key: string): string {
  return key
    .split('_')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ');
}
