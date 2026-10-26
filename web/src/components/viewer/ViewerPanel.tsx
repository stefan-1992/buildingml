import { useEffect, useRef, useCallback } from 'react';
import { WebGLRenderer } from '@shared/renderer/webgl-renderer.ts';
import { ThreeJSRenderer } from '@shared/renderer/threejs-renderer.ts';
import type { IRenderer } from '@shared/renderer/renderer-interface.ts';
import { OrbitCameraController } from '@shared/renderer/orbit-camera.ts';
import { getSceneStats } from '@shared/renderer/scene-loader.ts';
import { cutFraction, applyCutFraction } from '@shared/renderer/section-cut.ts';
import { useViewerStore } from '@state/viewer-store.ts';
import { useAppStore } from '@state/app-store.ts';
import { useFloorplanStore } from '@state/floorplan-store.ts';
import { useIsMobile } from '@shared/use-is-mobile.ts';
import { SectionCutSlider } from './SectionCutSlider.tsx';
import { Viewer3DPropertiesPanel } from './Viewer3DPropertiesPanel.tsx';

function createRenderer(engine: 'webgl' | 'threejs', canvas: HTMLCanvasElement): IRenderer {
  return engine === 'threejs' ? new ThreeJSRenderer(canvas) : new WebGLRenderer(canvas);
}

/** Threshold in pixels to distinguish click from drag. */
const DRAG_THRESHOLD = 4;

export function ViewerPanel() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<IRenderer | null>(null);
  const cameraCtrlRef = useRef<OrbitCameraController | null>(null);
  const isMobile = useIsMobile();

  // Drag-vs-click detection
  const mouseDownPos = useRef<{ x: number; y: number } | null>(null);

  const viewMode = useAppStore((s) => s.viewMode);
  const scene = useViewerStore((s) => s.scene);
  const ifcScene = useViewerStore((s) => s.ifcScene);
  const wireframe = useViewerStore((s) => s.wireframe);
  const sectionCut = useViewerStore((s) => s.sectionCut);
  const setSectionCut = useViewerStore((s) => s.setSectionCut);
  const lateralCut = useViewerStore((s) => s.lateralCut);
  const setLateralCut = useViewerStore((s) => s.setLateralCut);
  const setStatus = useAppStore((s) => s.setStatus);
  const rendererEngine = useViewerStore((s) => s.rendererEngine);
  const selectedElement = useViewerStore((s) => s.selectedElement);
  const setSelectedElement = useViewerStore((s) => s.setSelectedElement);

  // Initialize renderer — recreate when engine preference changes
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    try {
      const renderer = createRenderer(rendererEngine, canvas);
      rendererRef.current = renderer;

      const camCtrl = new OrbitCameraController(canvas);
      cameraCtrlRef.current = camCtrl;
      // Share camera state
      renderer.camera = camCtrl.camera;

      // Wire tap/long-press for element picking on mobile
      const pickAt = (clientX: number, clientY: number) => {
        const r = rendererRef.current;
        if (!r) return;
        const result = r.pick(clientX, clientY);
        useViewerStore.getState().setSelectedElement(result);
      };
      camCtrl.onTap = pickAt;
      camCtrl.onLongPress = pickAt;

      renderer.start();

      return () => {
        camCtrl.dispose();
        renderer.dispose();
        rendererRef.current = null;
        cameraCtrlRef.current = null;
      };
    } catch (e) {
      const label = rendererEngine === 'threejs' ? 'Three.js' : 'WebGL';
      setStatus(`${label} renderer not available`);
    }
  }, [setStatus, rendererEngine]);

  // Load scene when it changes (use IFC scene when in IFC mode)
  const activeScene = viewMode === 'ifc' ? ifcScene : scene;
  useEffect(() => {
    const renderer = rendererRef.current;
    const camCtrl = cameraCtrlRef.current;
    if (!renderer || !activeScene) return;

    renderer.loadScene(activeScene);
    // Sync camera state back to controller
    if (camCtrl) {
      camCtrl.camera.distance = renderer.camera.distance;
      camCtrl.camera.rotX = renderer.camera.rotX;
      camCtrl.camera.rotY = renderer.camera.rotY;
      camCtrl.camera.target = [...renderer.camera.target];
    }
    // Adopt the new scene's cut bounds, keeping each cut where the user had it
    // as a fraction of the model's extent. Read straight from the store rather
    // than through a dep, so changing a cut does not reload the scene.
    const prev = useViewerStore.getState();
    applyCutFraction(renderer.sectionCut, cutFraction(prev.sectionCut));
    applyCutFraction(renderer.lateralCut, cutFraction(prev.lateralCut));
    setSectionCut({
      min: renderer.sectionCut.min,
      max: renderer.sectionCut.max,
      value: renderer.sectionCut.value,
      step: renderer.sectionCut.step,
    });
    setLateralCut({
      min: renderer.lateralCut.min,
      max: renderer.lateralCut.max,
      value: renderer.lateralCut.value,
      step: renderer.lateralCut.step,
    });

    const stats = getSceneStats(activeScene);
    const prefix = viewMode === 'ifc' ? 'IFC: ' : '';
    const engine = rendererEngine === 'threejs' ? ' [Three.js]' : ' [WebGL]';
    setStatus(`${prefix}${stats.nodeCount} elements, ${stats.triCount} triangles${engine}`);
  }, [activeScene, viewMode, setSectionCut, setLateralCut, setStatus, rendererEngine]);

  // Sync wireframe + section cuts + highlight to renderer
  useEffect(() => {
    const renderer = rendererRef.current;
    if (!renderer) return;
    renderer.wireframe = wireframe;
    renderer.sectionCut = sectionCut;
    renderer.lateralCut = lateralCut;
    renderer.highlightedId = selectedElement?.id ?? null;
  }, [wireframe, sectionCut, lateralCut, selectedElement]);

  // Attach/detach gizmo when selection changes (Three.js only)
  useEffect(() => {
    const renderer = rendererRef.current;
    const camCtrl = cameraCtrlRef.current;
    if (!renderer) return;

    const elementId = selectedElement?.id ?? null;
    renderer.attachGizmo(elementId);

    // Wire up transform callback
    renderer.onTransform = (id, delta) => {
      useFloorplanStore.getState().nudgeElement(id, delta[0], delta[1]);
      setStatus(`Moved ${id}`);
    };

    // Sync gizmo drag state to orbit camera suppression via pointerdown/up
    // (replaces perpetual rAF loop — isDraggingGizmo is set by Three.js dragging-changed event)
    const syncDrag = () => {
      if (camCtrl) camCtrl.suppressDrag = renderer.isDraggingGizmo;
    };
    const canvas = canvasRef.current;
    canvas?.addEventListener('pointerdown', syncDrag);
    canvas?.addEventListener('pointerup', syncDrag);

    return () => {
      renderer.onTransform = null;
      if (camCtrl) camCtrl.suppressDrag = false;
      canvas?.removeEventListener('pointerdown', syncDrag);
      canvas?.removeEventListener('pointerup', syncDrag);
    };
  }, [selectedElement, setStatus]);

  const handleResetView = useCallback(() => {
    const camCtrl = cameraCtrlRef.current;
    if (camCtrl) camCtrl.reset();
  }, []);

  // Click-to-pick: detect mousedown/mouseup without drag
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    mouseDownPos.current = { x: e.clientX, y: e.clientY };
  }, []);

  const handleMouseUp = useCallback((e: React.MouseEvent) => {
    const down = mouseDownPos.current;
    mouseDownPos.current = null;
    if (!down) return;

    // Ignore if gizmo is being dragged
    const renderer = rendererRef.current;
    if (renderer?.isDraggingGizmo) return;

    const dx = Math.abs(e.clientX - down.x);
    const dy = Math.abs(e.clientY - down.y);
    if (dx > DRAG_THRESHOLD || dy > DRAG_THRESHOLD) return; // was a drag, not a click

    if (!renderer) return;

    const result = renderer.pick(e.clientX, e.clientY);
    setSelectedElement(result);
  }, [setSelectedElement]);

  // Rebuild 3D scene from current BML (after undo/redo)
  const rebuildScene = useCallback(async () => {
    const bml = useFloorplanStore.getState().bmlOutput;
    const provider = useAppStore.getState().provider;
    if (!provider || !bml) return;
    try {
      const newScene = await provider.generateScene(bml);
      if (newScene) useViewerStore.getState().setScene(newScene);
    } catch { /* rebuild failed silently */ }
  }, []);

  // Keyboard: Escape, Cmd+Z, Cmd+Shift+Z, Cmd+Y, Cmd+S
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;

      if (e.key === 'Escape' && selectedElement) {
        setSelectedElement(null);
        return;
      }
      if (e.ctrlKey || e.metaKey) {
        if (e.key === 'z' && !e.shiftKey) {
          e.preventDefault();
          useFloorplanStore.getState().undo();
          rebuildScene();
          return;
        }
        if (e.key === 'z' && e.shiftKey) {
          e.preventDefault();
          useFloorplanStore.getState().redo();
          rebuildScene();
          return;
        }
        if (e.key === 'y') {
          e.preventDefault();
          useFloorplanStore.getState().redo();
          rebuildScene();
          return;
        }
        if (e.key === 's') {
          e.preventDefault();
          useFloorplanStore.getState().saveBml();
          setStatus('Saved');
          return;
        }
        return;
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [selectedElement, setSelectedElement, setStatus, rebuildScene]);

  const helpText = isMobile
    ? 'Tap: select | Drag: rotate | Pinch: zoom | 2-finger: pan'
    : 'Click: select | Drag: rotate | Scroll: zoom | Shift+drag: pan';

  const showPropertiesPanel = !isMobile && selectedElement !== null;

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%', background: '#000', touchAction: 'none', display: 'flex' }}>
      <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
        <canvas
          key={rendererEngine}
          ref={canvasRef}
          style={{ width: '100%', height: '100%', display: 'block', touchAction: 'none' }}
          onMouseDown={handleMouseDown}
          onMouseUp={handleMouseUp}
        />
        <div style={{
          position: 'absolute', bottom: 8, left: 8,
          fontSize: isMobile ? 9 : 10, color: 'rgba(255,255,255,0.5)', pointerEvents: 'none',
        }}>
          {helpText}
        </div>
        {!isMobile && <SectionCutSlider />}
        <button
          onClick={handleResetView}
          style={{
            position: 'absolute', top: 8, right: 8,
            background: 'rgba(0,0,0,0.55)', border: 'none', color: 'rgba(255,255,255,0.75)',
            padding: isMobile ? '8px 14px' : '4px 8px', borderRadius: 4, cursor: 'pointer',
            fontFamily: 'monospace', fontSize: isMobile ? 13 : 11,
          }}
        >
          Reset View
        </button>
      </div>
      {showPropertiesPanel && (
        <div style={{
          width: 240, minWidth: 200,
          background: 'var(--surface)', borderLeft: '1px solid var(--border)',
          overflow: 'auto',
        }}>
          <Viewer3DPropertiesPanel />
        </div>
      )}
    </div>
  );
}
