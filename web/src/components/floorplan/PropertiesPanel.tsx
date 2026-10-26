import { useFloorplanStore } from '@state/floorplan-store.ts';
import { wallLength } from '@shared/floorplan/element-model.ts';
import type { WallElement, DoorElement, WindowElement, ColumnElement, SlabElement } from '@shared/floorplan/element-model.ts';

export function PropertiesPanel() {
  const elements = useFloorplanStore((s) => s.elements);
  const detectedSpaces = useFloorplanStore((s) => s.detectedSpaces);
  const selectedId = useFloorplanStore((s) => s.selectedId);
  const updateProperty = useFloorplanStore((s) => s.updateProperty);
  const renameSpace = useFloorplanStore((s) => s.renameSpace);
  const toggleAnchor = useFloorplanStore((s) => s.toggleAnchor);
  const saveState = useFloorplanStore((s) => s.saveState);

  const handleChange = (elemId: string, path: string, value: string | number) => {
    saveState();
    updateProperty(elemId, path, value);
  };

  // Space selected
  if (selectedId?.startsWith('space-')) {
    const idx = parseInt(selectedId.split('-')[1]);
    const space = detectedSpaces[idx];
    if (!space) return <NoSelection />;

    return (
      <Section title="Space">
        <PropRow label="Name">
          <input className="prop-input" value={space.name} onChange={(e) => renameSpace(idx, e.target.value)} style={inputStyle} />
        </PropRow>
        <PropRow label="Area">
          <input className="prop-input" disabled value={`${space.area.toFixed(2)} m\u00B2`} style={inputStyle} />
        </PropRow>
        <PropRow label="Walls">
          <input className="prop-input" disabled value={String(space.wallIds.length)} style={inputStyle} />
        </PropRow>
      </Section>
    );
  }

  const sel = selectedId ? elements.find(e => e.id === selectedId) : null;
  if (!sel) return <NoSelection />;

  return (
    <div>
      <Section title={sel.type}>
        <PropRow label="ID">
          <input style={inputStyle} value={sel.id} onChange={(e) => handleChange(sel.id, 'id', e.target.value)} />
        </PropRow>

        {sel.type === 'Wall' && <WallProps wall={sel as WallElement} onChange={handleChange} onToggleAnchor={toggleAnchor} />}
        {sel.type === 'Door' && <DoorProps door={sel as DoorElement} onChange={handleChange} />}
        {sel.type === 'Window' && <WindowProps win={sel as WindowElement} onChange={handleChange} />}
        {sel.type === 'Column' && <ColumnProps col={sel as ColumnElement} onChange={handleChange} />}
        {sel.type === 'Slab' && <SlabProps slab={sel as SlabElement} onChange={handleChange} />}
      </Section>
    </div>
  );
}

function WallProps({ wall, onChange, onToggleAnchor }: { wall: WallElement; onChange: (id: string, path: string, v: number) => void; onToggleAnchor: () => void }) {
  return (
    <>
      <PropRow label="From X"><NumInput value={wall.from.x} step={0.25} onChange={(v) => onChange(wall.id, 'from.x', v)} /></PropRow>
      <PropRow label="From Y"><NumInput value={wall.from.y} step={0.25} onChange={(v) => onChange(wall.id, 'from.y', v)} /></PropRow>
      <PropRow label="To X"><NumInput value={wall.to.x} step={0.25} onChange={(v) => onChange(wall.id, 'to.x', v)} /></PropRow>
      <PropRow label="To Y"><NumInput value={wall.to.y} step={0.25} onChange={(v) => onChange(wall.id, 'to.y', v)} /></PropRow>
      <PropRow label="Height"><NumInput value={wall.height} step={0.1} onChange={(v) => onChange(wall.id, 'height', v)} /></PropRow>
      <PropRow label="Thickness"><NumInput value={wall.thickness} step={0.01} onChange={(v) => onChange(wall.id, 'thickness', v)} /></PropRow>
      <PropRow label="Length"><input style={inputStyle} disabled value={`${wallLength(wall).toFixed(3)} m`} /></PropRow>
      <PropRow label="Anchored">
        <input type="checkbox" checked={wall.anchored} onChange={onToggleAnchor} style={{ accentColor: 'var(--error)' }} />
      </PropRow>
    </>
  );
}

function DoorProps({ door, onChange }: { door: DoorElement; onChange: (id: string, path: string, v: number) => void }) {
  return (
    <>
      <PropRow label="Wall"><input style={inputStyle} disabled value={door.parentWall} /></PropRow>
      <PropRow label="Offset"><NumInput value={door.offset} step={0.25} onChange={(v) => onChange(door.id, 'offset', v)} /></PropRow>
      <PropRow label="Width"><NumInput value={door.width} step={0.1} onChange={(v) => onChange(door.id, 'width', v)} /></PropRow>
      <PropRow label="Height"><NumInput value={door.height} step={0.1} onChange={(v) => onChange(door.id, 'height', v)} /></PropRow>
    </>
  );
}

function WindowProps({ win, onChange }: { win: WindowElement; onChange: (id: string, path: string, v: number) => void }) {
  return (
    <>
      <PropRow label="Wall"><input style={inputStyle} disabled value={win.parentWall} /></PropRow>
      <PropRow label="Offset"><NumInput value={win.offset} step={0.25} onChange={(v) => onChange(win.id, 'offset', v)} /></PropRow>
      <PropRow label="Width"><NumInput value={win.width} step={0.1} onChange={(v) => onChange(win.id, 'width', v)} /></PropRow>
      <PropRow label="Height"><NumInput value={win.height} step={0.1} onChange={(v) => onChange(win.id, 'height', v)} /></PropRow>
      <PropRow label="Sill"><NumInput value={win.sill_height} step={0.1} onChange={(v) => onChange(win.id, 'sill_height', v)} /></PropRow>
    </>
  );
}

function ColumnProps({ col, onChange }: { col: ColumnElement; onChange: (id: string, path: string, v: number) => void }) {
  return (
    <>
      <PropRow label="X"><NumInput value={col.x} step={0.25} onChange={(v) => onChange(col.id, 'x', v)} /></PropRow>
      <PropRow label="Y"><NumInput value={col.y} step={0.25} onChange={(v) => onChange(col.id, 'y', v)} /></PropRow>
      <PropRow label="Height"><NumInput value={col.height} step={0.1} onChange={(v) => onChange(col.id, 'height', v)} /></PropRow>
      <PropRow label="Width"><NumInput value={col.profileW} step={0.05} onChange={(v) => onChange(col.id, 'profileW', v)} /></PropRow>
      <PropRow label="Depth"><NumInput value={col.profileD} step={0.05} onChange={(v) => onChange(col.id, 'profileD', v)} /></PropRow>
    </>
  );
}

function SlabProps({ slab, onChange }: { slab: SlabElement; onChange: (id: string, path: string, v: number) => void }) {
  return (
    <>
      <PropRow label="Thickness"><NumInput value={slab.thickness} step={0.01} onChange={(v) => onChange(slab.id, 'thickness', v)} /></PropRow>
      <PropRow label="Points"><input style={inputStyle} disabled value={`${slab.boundary.length} vertices`} /></PropRow>
    </>
  );
}

function NoSelection() {
  return (
    <div style={{ padding: '8px 12px', color: 'var(--text-dim)', fontSize: 12 }}>
      Select an element to edit its properties
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border)' }}>
      <div style={{ fontSize: 10, fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5, color: 'var(--text-dim)', marginBottom: 6 }}>
        {title}
      </div>
      {children}
    </div>
  );
}

function PropRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
      <span style={{ fontSize: 11, color: 'var(--text-dim)', minWidth: 70, flexShrink: 0 }}>{label}</span>
      {children}
    </div>
  );
}

function NumInput({ value, step, onChange }: { value: number; step: number; onChange: (v: number) => void }) {
  return (
    <input
      type="number"
      step={step}
      value={value}
      onChange={(e) => onChange(parseFloat(e.target.value))}
      style={inputStyle}
    />
  );
}

const inputStyle: React.CSSProperties = {
  flex: 1, background: 'var(--bg)', border: '1px solid var(--border)',
  color: 'var(--text)', padding: '3px 6px', borderRadius: 3,
  fontFamily: 'inherit', fontSize: 11, outline: 'none', minWidth: 0,
};
