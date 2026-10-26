import { useCallback } from 'react';
import { useFloorplanStore } from '@state/floorplan-store.ts';

const colorMap: Record<string, string> = {
  Wall: '#c8c0b8', Door: '#a06020', Window: '#5588cc',
  Column: '#888886', Slab: '#d0d0ce', Beam: '#666',
};

export function ElementList() {
  const elements = useFloorplanStore((s) => s.elements);
  const detectedSpaces = useFloorplanStore((s) => s.detectedSpaces);
  const selectedId = useFloorplanStore((s) => s.selectedId);
  const setSelectedId = useFloorplanStore((s) => s.setSelectedId);

  const handleDoubleClick = useCallback((id: string) => {
    const s = useFloorplanStore.getState();
    if (id.startsWith('space-')) {
      s.zoomToSpace(id);
    } else {
      s.zoomToElement(id);
    }
  }, []);

  return (
    <div style={{
      background: 'var(--surface)', display: 'flex', flexDirection: 'column', overflow: 'hidden',
    }}>
      <div style={{
        padding: '8px 12px', fontSize: 11, color: 'var(--text-dim)',
        borderBottom: '1px solid var(--border)',
        fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5,
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
      }}>
        <span>Elements</span>
        <span style={{ fontWeight: 400, fontSize: 10, opacity: 0.7 }}>
          {detectedSpaces.length} spaces, {elements.length} elements
        </span>
      </div>
      <div style={{ flex: 1, overflowY: 'auto', padding: '4px 0' }}>
        {detectedSpaces.map((space) => (
          <div
            key={space.id}
            onClick={() => setSelectedId(space.id)}
            onDoubleClick={() => handleDoubleClick(space.id)}
            style={{
              padding: '6px 12px', fontSize: 12, cursor: 'pointer',
              display: 'flex', alignItems: 'center', gap: 8,
              borderLeft: space.id === selectedId ? '3px solid var(--accent)' : '3px solid transparent',
              background: space.id === selectedId ? 'rgba(137,180,250,0.1)' : undefined,
              color: space.id === selectedId ? 'var(--accent)' : undefined,
            }}
          >
            <div style={{
              width: 14, height: 14, borderRadius: 2, flexShrink: 0,
              background: 'rgba(137,180,250,0.3)', border: '1px solid rgba(137,180,250,0.5)',
            }} />
            <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{space.name}</span>
            <span style={{ fontSize: 10, color: 'var(--text-dim)' }}>Space</span>
          </div>
        ))}
        {elements.map((e) => (
          <div
            key={e.id}
            onClick={() => setSelectedId(e.id)}
            onDoubleClick={() => handleDoubleClick(e.id)}
            style={{
              padding: '6px 12px', fontSize: 12, cursor: 'pointer',
              display: 'flex', alignItems: 'center', gap: 8,
              borderLeft: e.id === selectedId ? '3px solid var(--accent)' : '3px solid transparent',
              background: e.id === selectedId ? 'rgba(137,180,250,0.1)' : undefined,
              color: e.id === selectedId ? 'var(--accent)' : undefined,
            }}
          >
            <div style={{
              width: 14, height: 14, borderRadius: 2, flexShrink: 0,
              background: colorMap[e.type] || '#888',
            }} />
            <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{e.id}</span>
            <span style={{ fontSize: 10, color: 'var(--text-dim)' }}>{e.type}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
