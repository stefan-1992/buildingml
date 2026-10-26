import { useCallback, useEffect, useState } from 'react';
import { useAppStore, type ViewMode } from '@state/app-store.ts';
import { useViewerStore } from '@state/viewer-store.ts';
import { useFloorplanStore } from '@state/floorplan-store.ts';
import { useChatStore } from '@state/chat-store.ts';
import { useBreakpoint } from '@shared/use-breakpoint.ts';
import { initIfcApi, ifcToSceneGraph } from '@shared/renderer/ifc-loader.ts';

interface Example {
  name: string;
  file: string;
}

export function Toolbar() {
  const viewMode = useAppStore((s) => s.viewMode);
  const setViewMode = useAppStore((s) => s.setViewMode);
  const source = useAppStore((s) => s.source);
  const setSource = useAppStore((s) => s.setSource);
  const provider = useAppStore((s) => s.provider);
  const status = useAppStore((s) => s.status);
  const setStatus = useAppStore((s) => s.setStatus);
  const setDiagnostics = useAppStore((s) => s.setDiagnostics);
  const setBuilding = useAppStore((s) => s.setBuilding);
  const exampleName = useAppStore((s) => s.exampleName);
  const setExampleName = useAppStore((s) => s.setExampleName);
  const setScene = useViewerStore((s) => s.setScene);
  const setIfcScene = useViewerStore((s) => s.setIfcScene);
  const toggleWireframe = useViewerStore((s) => s.toggleWireframe);
  const rendererEngine = useViewerStore((s) => s.rendererEngine);
  const toggleRendererEngine = useViewerStore((s) => s.toggleRendererEngine);
  const chatOpen = useChatStore((s) => s.isOpen);
  const toggleChat = useChatStore((s) => s.toggleOpen);
  const breakpoint = useBreakpoint();

  const [examples, setExamples] = useState<Example[]>([]);
  const [ifcLoading, setIfcLoading] = useState(false);

  // Fetch available examples on mount
  useEffect(() => {
    (async () => {
      try {
        const res = await fetch('/api/examples');
        if (res.ok) {
          const data: Example[] = await res.json();
          setExamples(data);
        }
      } catch {
        // Examples API not available
      }
    })();
  }, []);

  const handleBuild = useCallback(async (sourceOverride?: string) => {
    const buildSource = sourceOverride ?? source;
    setBuilding(true);
    setStatus('Building...');
    try {
      const [scene, diags] = await Promise.all([
        provider.generateScene(buildSource),
        provider.lint(buildSource),
      ]);
      setScene(scene);
      setDiagnostics(diags);
    } catch (e) {
      setStatus('Build failed');
      setDiagnostics([{
        code: 'E001',
        severity: 'error',
        message: e instanceof Error ? e.message : 'Unknown error',
        help: null,
      }]);
    } finally {
      setBuilding(false);
    }
  }, [source, provider, setScene, setStatus, setDiagnostics, setBuilding]);

  const handleIfcPreview = useCallback(async () => {
    setIfcLoading(true);
    setStatus('Compiling IFC...');
    try {
      const ifcContent = await provider.compile(source);
      setStatus('Loading IFC geometry...');
      const api = await initIfcApi('/');
      const scene = ifcToSceneGraph(api, ifcContent);
      setIfcScene(scene);
      setViewMode('ifc');
      setStatus(`IFC: ${scene.nodes.length} elements`);
    } catch (e) {
      setStatus(`IFC error: ${e instanceof Error ? e.message : 'Unknown error'}`);
    } finally {
      setIfcLoading(false);
    }
  }, [source, provider, setIfcScene, setViewMode, setStatus]);

  const handleExportIfc = useCallback(async () => {
    setStatus('Compiling IFC for export...');
    try {
      const ifcContent = await provider.compile(source);
      const blob = new Blob([ifcContent], { type: 'application/x-step' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'building.ifc';
      a.click();
      URL.revokeObjectURL(url);
      setStatus('IFC exported');
    } catch (e) {
      setStatus(`Export failed: ${e instanceof Error ? e.message : 'Unknown error'}`);
    }
  }, [source, provider, setStatus]);

  const handleExampleChange = useCallback(async (file: string) => {
    setExampleName(file);
    setStatus('Loading example...');
    try {
      const res = await fetch(`/api/examples/${file}`);
      if (!res.ok) throw new Error(`Failed to load example: ${res.statusText}`);
      const content = await res.text();
      setSource(content);

      // Reload floorplan model
      try {
        const model = await provider.generateModel(content);
        useFloorplanStore.getState().setOriginalFileSource(content);
        useFloorplanStore.getState().loadFromModel(model, 0);
      } catch (e) {
        console.warn('Floorplan model load failed:', e);
      }

      // Rebuild with new source
      handleBuild(content);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : 'Failed to load example');
    }
  }, [provider, setSource, setExampleName, setStatus, handleBuild]);

  const isPhone = breakpoint === 'phone';
  const isTablet = breakpoint === 'tablet';

  // Phone: only 3D, 2D, IFC (no split)
  const viewButtons: { mode: ViewMode; label: string }[] = isPhone
    ? [
        { mode: '3d', label: '3D' },
        { mode: '2d', label: '2D' },
        { mode: 'ifc', label: 'IFC' },
      ]
    : [
        { mode: '3d', label: '3D' },
        { mode: '2d', label: '2D' },
        { mode: 'split', label: 'Split' },
        { mode: 'ifc', label: 'IFC' },
      ];

  const handleViewModeClick = useCallback((mode: ViewMode) => {
    if (mode === 'ifc') {
      handleIfcPreview();
    } else {
      setViewMode(mode);
    }
  }, [handleIfcPreview, setViewMode]);

  // --- Phone toolbar: minimal ---
  if (isPhone) {
    return (
      <div style={{
        background: 'var(--surface)', display: 'flex',
        alignItems: 'center', padding: '0 8px', gap: 6,
        height: 44, borderBottom: '1px solid var(--border)',
        minHeight: 44,
      }}>
        <h1 style={{ fontSize: 13, fontWeight: 600, color: 'var(--accent)', margin: 0 }}>
          BML
        </h1>

        {examples.length > 0 && (
          <select
            value={exampleName}
            onChange={(e) => handleExampleChange(e.target.value)}
            style={{
              background: 'var(--border)', border: 'none', color: 'var(--text)',
              padding: '6px 8px', borderRadius: 4, cursor: 'pointer',
              fontFamily: 'inherit', fontSize: 12, maxWidth: 120,
              minHeight: 32,
            }}
          >
            {examples.map(({ name, file }) => (
              <option key={file} value={file}>{name}</option>
            ))}
          </select>
        )}

        <button onClick={() => handleBuild()} style={mobileBtnStyle}>Build</button>
        <button
          onClick={toggleRendererEngine}
          style={{
            ...mobileBtnStyle,
            background: rendererEngine === 'threejs' ? 'var(--accent)' : 'var(--border)',
            color: rendererEngine === 'threejs' ? 'var(--bg)' : 'var(--text)',
          }}
        >
          {rendererEngine === 'threejs' ? '3JS' : 'GL'}
        </button>

        <span style={{ marginLeft: 'auto', fontSize: 10, color: 'var(--text-dim)', maxWidth: 80, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {status}
        </span>
      </div>
    );
  }

  // --- Tablet toolbar: compact ---
  if (isTablet) {
    return (
      <div style={{
        background: 'var(--surface)', display: 'flex',
        alignItems: 'center', padding: '0 10px', gap: 8,
        height: 42, borderBottom: '1px solid var(--border)',
      }}>
        <h1 style={{ fontSize: 13, fontWeight: 600, color: 'var(--accent)', margin: 0 }}>
          BML
        </h1>

        {examples.length > 0 && (
          <select
            value={exampleName}
            onChange={(e) => handleExampleChange(e.target.value)}
            style={{
              background: 'var(--border)', border: 'none', color: 'var(--text)',
              padding: '4px 8px', borderRadius: 4, cursor: 'pointer',
              fontFamily: 'inherit', fontSize: 12, maxWidth: 160,
            }}
          >
            {examples.map(({ name, file }) => (
              <option key={file} value={file}>{name}</option>
            ))}
          </select>
        )}

        {viewButtons.map(({ mode, label }) => (
          <button
            key={mode}
            onClick={() => handleViewModeClick(mode)}
            disabled={mode === 'ifc' && ifcLoading}
            style={{
              background: viewMode === mode ? 'var(--accent)' : 'var(--border)',
              color: viewMode === mode ? 'var(--bg)' : 'var(--text)',
              border: 'none', padding: '4px 10px', borderRadius: 4,
              cursor: 'pointer', fontFamily: 'inherit', fontSize: 12,
              opacity: mode === 'ifc' && ifcLoading ? 0.5 : 1,
            }}
          >
            {label}
          </button>
        ))}

        <div style={{ width: 1, height: 20, background: 'var(--border)' }} />

        <button onClick={() => handleBuild()} style={tabletBtnStyle}>Build</button>
        <button onClick={toggleWireframe} style={tabletBtnStyle}>Wire</button>
        <button
          onClick={toggleRendererEngine}
          style={{
            ...tabletBtnStyle,
            background: rendererEngine === 'threejs' ? 'var(--accent)' : 'var(--border)',
            color: rendererEngine === 'threejs' ? 'var(--bg)' : 'var(--text)',
          }}
        >
          {rendererEngine === 'threejs' ? '3JS' : 'GL'}
        </button>
        <button onClick={handleExportIfc} style={tabletBtnStyle}>IFC</button>
        <button
          onClick={toggleChat}
          style={{
            ...tabletBtnStyle,
            background: chatOpen ? 'var(--accent)' : 'var(--border)',
            color: chatOpen ? 'var(--bg)' : 'var(--text)',
          }}
        >
          Chat
        </button>

        <span style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--text-dim)' }}>
          {status}
        </span>
      </div>
    );
  }

  // --- Desktop toolbar ---
  return (
    <div style={{
      background: 'var(--surface)', display: 'flex',
      alignItems: 'center', padding: '0 12px', gap: 12,
      height: 40, borderBottom: '1px solid var(--border)',
    }}>
      <h1 style={{ fontSize: 14, fontWeight: 600, color: 'var(--accent)', margin: 0 }}>
        BuildingML
      </h1>

      {examples.length > 0 && (
        <select
          value={exampleName}
          onChange={(e) => handleExampleChange(e.target.value)}
          style={{
            background: 'var(--border)', border: 'none', color: 'var(--text)',
            padding: '4px 8px', borderRadius: 4, cursor: 'pointer',
            fontFamily: 'inherit', fontSize: 12, maxWidth: 200,
          }}
        >
          {examples.map(({ name, file }) => (
            <option key={file} value={file}>{name}</option>
          ))}
        </select>
      )}

      {viewButtons.map(({ mode, label }) => (
        <button
          key={mode}
          onClick={() => handleViewModeClick(mode)}
          disabled={mode === 'ifc' && ifcLoading}
          style={{
            background: viewMode === mode ? 'var(--accent)' : 'var(--border)',
            color: viewMode === mode ? 'var(--bg)' : 'var(--text)',
            border: 'none', padding: '4px 12px', borderRadius: 4,
            cursor: 'pointer', fontFamily: 'inherit', fontSize: 12,
            opacity: mode === 'ifc' && ifcLoading ? 0.5 : 1,
          }}
        >
          {label}
        </button>
      ))}

      <div style={{ width: 1, height: 20, background: 'var(--border)' }} />

      <button onClick={() => handleBuild()} style={btnStyle}>Build</button>
      <button onClick={toggleWireframe} style={btnStyle}>Wireframe</button>
      <button
        onClick={toggleRendererEngine}
        title={`Switch to ${rendererEngine === 'webgl' ? 'Three.js' : 'WebGL'} renderer`}
        style={{
          ...btnStyle,
          background: rendererEngine === 'threejs' ? 'var(--accent)' : 'var(--border)',
          color: rendererEngine === 'threejs' ? 'var(--bg)' : 'var(--text)',
        }}
      >
        {rendererEngine === 'threejs' ? 'Three.js' : 'WebGL'}
      </button>
      <button onClick={handleExportIfc} style={btnStyle}>Export IFC</button>
      <button
        onClick={toggleChat}
        style={{
          ...btnStyle,
          background: chatOpen ? 'var(--accent)' : 'var(--border)',
          color: chatOpen ? 'var(--bg)' : 'var(--text)',
        }}
      >
        Chat
      </button>

      <span style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--text-dim)' }}>
        {status}
      </span>
    </div>
  );
}

const btnStyle: React.CSSProperties = {
  background: 'var(--border)', border: 'none', color: 'var(--text)',
  padding: '4px 12px', borderRadius: 4, cursor: 'pointer',
  fontFamily: 'inherit', fontSize: 12,
};

const tabletBtnStyle: React.CSSProperties = {
  background: 'var(--border)', border: 'none', color: 'var(--text)',
  padding: '4px 10px', borderRadius: 4, cursor: 'pointer',
  fontFamily: 'inherit', fontSize: 12,
};

const mobileBtnStyle: React.CSSProperties = {
  background: 'var(--border)', border: 'none', color: 'var(--text)',
  padding: '6px 14px', borderRadius: 4, cursor: 'pointer',
  fontFamily: 'inherit', fontSize: 13, minHeight: 32,
};
