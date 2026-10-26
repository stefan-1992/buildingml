import { useFloorplanStore, type FloorplanTool } from '@state/floorplan-store.ts';
import { getStoreys } from '@shared/floorplan/model-importer.ts';
import { useRef, useCallback, useEffect } from 'react';

const tools: { tool: FloorplanTool; label: string; shortcut: string }[] = [
  { tool: 'select', label: 'Select', shortcut: 'V' },
  { tool: 'wall', label: 'Wall', shortcut: 'W' },
  { tool: 'door', label: 'Door', shortcut: 'D' },
  { tool: 'window', label: 'Window', shortcut: 'N' },
  { tool: 'column', label: 'Column', shortcut: 'C' },
  { tool: 'slab', label: 'Slab', shortcut: 'S' },
  { tool: 'furniture', label: 'Furniture', shortcut: 'U' },
];

export function ToolSelector() {
  const currentTool = useFloorplanStore((s) => s.currentTool);
  const setTool = useFloorplanStore((s) => s.setTool);
  const showGrid = useFloorplanStore((s) => s.showGrid);
  const snapEnabled = useFloorplanStore((s) => s.snapEnabled);
  const hasUnsavedChanges = useFloorplanStore((s) => s.hasUnsavedChanges);
  const loadedModel = useFloorplanStore((s) => s.loadedModel);
  const currentStoreyIndex = useFloorplanStore((s) => s.currentStoreyIndex);
  const selectedAsset = useFloorplanStore((s) => s.selectedAsset);
  const setSelectedAsset = useFloorplanStore((s) => s.setSelectedAsset);
  const canvasRef = useRef<HTMLDivElement>(null);

  const handleUndo = useCallback(() => useFloorplanStore.getState().undo(), []);
  const handleRedo = useCallback(() => useFloorplanStore.getState().redo(), []);
  const handleSave = useCallback(() => useFloorplanStore.getState().saveBml(), []);
  const handleToggleGrid = useCallback(() => useFloorplanStore.getState().toggleGrid(), []);
  const handleToggleSnap = useCallback(() => useFloorplanStore.getState().toggleSnap(), []);

  const handleZoomToFit = useCallback(() => {
    // Find canvas element to get dimensions
    const canvas = document.querySelector('canvas');
    if (canvas) {
      const rect = canvas.getBoundingClientRect();
      useFloorplanStore.getState().zoomToFit(rect.width, rect.height);
    }
  }, []);

  const handleStoreyChange = useCallback((e: React.ChangeEvent<HTMLSelectElement>) => {
    const s = useFloorplanStore.getState();
    if (s.loadedModel) {
      s.loadFromModel(s.loadedModel, parseInt(e.target.value));
    }
  }, []);

  const storeys = loadedModel ? getStoreys(loadedModel) : [];

  // Collect available assets from all asset libraries in the loaded model
  const availableAssets: string[] = [];
  for (const lib of loadedModel?.asset_libraries || []) {
    for (const asset of lib.assets || []) {
      availableAssets.push(asset.id);
    }
  }

  // Auto-select first asset when switching to furniture tool
  useEffect(() => {
    if (currentTool === 'furniture' && !selectedAsset && availableAssets.length > 0) {
      setSelectedAsset(availableAssets[0]);
    }
  }, [currentTool, selectedAsset, availableAssets, setSelectedAsset]);

  return (
    <div ref={canvasRef} style={{ display: 'flex', gap: 2, alignItems: 'center', flexWrap: 'wrap' }}>
      {tools.map(({ tool, label, shortcut }) => (
        <button
          key={tool}
          onClick={() => setTool(tool)}
          title={`${label} (${shortcut})`}
          style={{
            background: currentTool === tool ? 'rgba(137,180,250,0.1)' : 'transparent',
            border: currentTool === tool ? '1px solid var(--accent)' : '1px solid transparent',
            color: currentTool === tool ? 'var(--accent)' : 'var(--text-dim)',
            padding: '4px 10px', borderRadius: 4, cursor: 'pointer',
            fontFamily: 'inherit', fontSize: 11, whiteSpace: 'nowrap',
          }}
        >
          {label}
        </button>
      ))}

      <Separator />

      <IconBtn title="Undo (Ctrl+Z)" onClick={handleUndo}>&#x21B6;</IconBtn>
      <IconBtn title="Redo (Ctrl+Y)" onClick={handleRedo}>&#x21B7;</IconBtn>
      <IconBtn title="Save (Ctrl+S)" onClick={handleSave} highlight={hasUnsavedChanges}>&#x1F4BE;</IconBtn>

      <Separator />

      <IconBtn title="Zoom to Fit (F)" onClick={handleZoomToFit}>&#x2922;</IconBtn>
      <IconBtn title="Toggle Grid (G)" onClick={handleToggleGrid} active={showGrid}>&#x23F9;</IconBtn>
      <IconBtn title="Toggle Snap (X)" onClick={handleToggleSnap} active={snapEnabled}>&#x2A2F;</IconBtn>

      {currentTool === 'furniture' && availableAssets.length > 0 && (
        <>
          <Separator />
          <select
            value={selectedAsset}
            onChange={(e) => setSelectedAsset(e.target.value)}
            title="Select asset to place"
            style={{
              background: 'var(--surface, #1e1e2e)', color: 'var(--text, #cdd6f4)',
              border: '1px solid var(--accent)', borderRadius: 4,
              padding: '3px 6px', fontSize: 11, fontFamily: 'inherit',
              cursor: 'pointer', outline: 'none',
            }}
          >
            {availableAssets.map((id) => (
              <option key={id} value={id}>{id}</option>
            ))}
          </select>
        </>
      )}

      {storeys.length > 1 && (
        <>
          <Separator />
          <select
            value={currentStoreyIndex}
            onChange={handleStoreyChange}
            title="Select storey"
            style={{
              background: 'var(--surface, #1e1e2e)', color: 'var(--text, #cdd6f4)',
              border: '1px solid var(--border, #444)', borderRadius: 4,
              padding: '3px 6px', fontSize: 11, fontFamily: 'inherit',
              cursor: 'pointer', outline: 'none',
            }}
          >
            {storeys.map((storey: { name: string }, i: number) => (
              <option key={i} value={i}>{storey.name || `Storey ${i + 1}`}</option>
            ))}
          </select>
        </>
      )}
    </div>
  );
}

function Separator() {
  return <div style={{ width: 1, height: 20, background: 'var(--border, #444)', margin: '0 4px' }} />;
}

function IconBtn({ title, onClick, children, active, highlight }: {
  title: string; onClick: () => void; children: React.ReactNode;
  active?: boolean; highlight?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      style={{
        background: active ? 'rgba(137,180,250,0.15)' : 'transparent',
        border: highlight ? '1px solid #f5a623' : active ? '1px solid var(--accent)' : '1px solid transparent',
        color: highlight ? '#f5a623' : active ? 'var(--accent)' : 'var(--text-dim)',
        padding: '4px 8px', borderRadius: 4, cursor: 'pointer',
        fontFamily: 'inherit', fontSize: 13, lineHeight: 1,
      }}
    >
      {children}
    </button>
  );
}
