import { useEffect, useCallback, useState } from 'react';
import { useAppStore, DEFAULT_EXAMPLE } from '@state/app-store.ts';
import { useViewerStore } from '@state/viewer-store.ts';
import { useFloorplanStore } from '@state/floorplan-store.ts';
import { useChatStore } from '@state/chat-store.ts';
import { useBreakpoint } from '@shared/use-breakpoint.ts';
import { Toolbar } from './layout/Toolbar.tsx';
import { BottomNav, type MobileTab } from './layout/BottomNav.tsx';
import { BottomSheet, type SheetState } from './layout/BottomSheet.tsx';
import { ViewerPanel } from './viewer/ViewerPanel.tsx';
import { CodeEditor } from './editor/CodeEditor.tsx';
import { DiagnosticsPanel } from './editor/DiagnosticsPanel.tsx';
import { FloorplanPanel } from './floorplan/FloorplanPanel.tsx';
import { ToolSelector } from './floorplan/ToolSelector.tsx';
import { ElementTree } from './floorplan/ElementTree.tsx';
import { PropertiesPanel } from './floorplan/PropertiesPanel.tsx';
import { BmlEditor } from './floorplan/BmlEditor.tsx';
import { ChatPanel } from './chat/ChatPanel.tsx';

const appFont = "'SF Mono', 'Fira Code', 'Cascadia Code', monospace";

export function App() {
  const viewMode = useAppStore((s) => s.viewMode);
  const source = useAppStore((s) => s.source);
  const setSource = useAppStore((s) => s.setSource);
  const provider = useAppStore((s) => s.provider);
  const setStatus = useAppStore((s) => s.setStatus);
  const setDiagnostics = useAppStore((s) => s.setDiagnostics);
  const setBuilding = useAppStore((s) => s.setBuilding);
  const setScene = useViewerStore((s) => s.setScene);
  const chatOpen = useChatStore((s) => s.isOpen);
  const breakpoint = useBreakpoint();

  // Mobile-specific state
  const [mobileTab, setMobileTab] = useState<MobileTab>('3d');
  const [sheetState, setSheetState] = useState<SheetState>('collapsed');

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

  // Load source from server on mount
  useEffect(() => {
    (async () => {
      let currentSource = source;

      // Try loading the default example first
      try {
        const res = await fetch(`/api/examples/${DEFAULT_EXAMPLE}`);
        if (res.ok) {
          const exampleSource = await res.text();
          if (exampleSource.length > 0) {
            setSource(exampleSource);
            currentSource = exampleSource;
          }
        }
      } catch {
        // Examples API not available, try server source
      }

      // If no example loaded, try server source
      if (currentSource === source) {
        try {
          const serverSource = await provider.getSource();
          if (serverSource.length > 0) {
            setSource(serverSource);
            currentSource = serverSource;
          }
        } catch {
          // No server available, use default source
        }
      }

      // Try loading model for floorplan
      try {
        const model = await provider.generateModel(currentSource);
        useFloorplanStore.getState().setOriginalFileSource(currentSource);
        useFloorplanStore.getState().loadFromModel(model, 0);
      } catch {
        useFloorplanStore.getState().loadExample();
      }

      // Auto-build on load with the fetched source
      handleBuild(currentSource);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Ctrl+Enter to build
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault();
        handleBuild();
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [handleBuild]);

  // Close bottom sheet when switching tabs on mobile
  const handleMobileTabChange = useCallback((tab: MobileTab) => {
    setMobileTab(tab);
    // Properties and chat open as bottom sheets; others close the sheet
    if (tab === 'properties') {
      setSheetState('half');
    } else if (tab === 'chat') {
      setSheetState('half');
    } else {
      setSheetState('collapsed');
    }
  }, []);

  // --- Phone layout: full-screen viewer + bottom nav + bottom sheets ---
  if (breakpoint === 'phone') {
    return (
      <div style={{
        display: 'flex', flexDirection: 'column',
        height: '100vh', width: '100vw', overflow: 'hidden',
        fontFamily: appFont, background: 'var(--bg)', color: 'var(--text)',
      }}>
        <Toolbar />
        <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
          {/* Main content area based on active tab */}
          {mobileTab === '3d' && <ViewerPanel />}
          {mobileTab === '2d' && <FloorplanPanel />}
          {mobileTab === 'code' && (
            <div style={{ height: '100%', overflow: 'auto' }}>
              <BmlEditor />
            </div>
          )}
          {/* Chat and properties are always rendered for bottom sheet but hidden behind main content */}
          {mobileTab !== 'chat' && mobileTab !== 'properties' && null}

          {/* Chat as bottom sheet */}
          {mobileTab === 'chat' && (
            <>
              <ViewerPanel />
              <BottomSheet
                title="Chat"
                state={sheetState}
                onStateChange={setSheetState}
              >
                <ChatPanel />
              </BottomSheet>
            </>
          )}

          {/* Properties as bottom sheet */}
          {mobileTab === 'properties' && (
            <>
              {viewMode === '2d' ? <FloorplanPanel /> : <ViewerPanel />}
              <BottomSheet
                title="Properties"
                state={sheetState}
                onStateChange={setSheetState}
              >
                <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
                  <ElementTree />
                  <PropertiesPanel />
                </div>
              </BottomSheet>
            </>
          )}

          {/* Legacy: full-screen chat overlay (from chat toggle in toolbar) */}
          {chatOpen && mobileTab !== 'chat' && (
            <div style={{
              position: 'absolute', inset: 0, zIndex: 100,
              background: 'var(--surface)',
            }}>
              <ChatPanel />
            </div>
          )}
        </div>
        <BottomNav
          activeTab={mobileTab}
          onTabChange={handleMobileTabChange}
          viewMode={viewMode}
        />
      </div>
    );
  }

  // --- Tablet layout: collapsible split panels ---
  if (breakpoint === 'tablet') {
    const chatSidebar = chatOpen ? (
      <div style={{ width: 280, minWidth: 240, overflow: 'hidden' }}>
        <ChatPanel />
      </div>
    ) : null;

    if (viewMode === '2d') {
      return (
        <div style={{
          display: 'flex', flexDirection: 'column',
          height: '100vh', overflow: 'hidden',
          fontFamily: appFont, background: 'var(--bg)', color: 'var(--text)',
        }}>
          <Toolbar />
          <div style={{
            display: 'flex', alignItems: 'center', padding: '0 12px', gap: 8,
            height: 36, background: 'var(--surface)', borderBottom: '1px solid var(--border)',
          }}>
            <ToolSelector />
          </div>
          <div style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>
            <div style={{
              flex: 1, display: 'grid',
              gridTemplateColumns: '200px 1fr 240px',
              gap: 1, background: 'var(--border)',
              overflow: 'hidden',
            }}>
              <ElementTree />
              <FloorplanPanel />
              <div style={{ background: 'var(--surface)', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
                <PropertiesPanel />
                <BmlEditor />
              </div>
            </div>
            {chatSidebar}
          </div>
        </div>
      );
    }

    const show3d = viewMode === '3d' || viewMode === 'split' || viewMode === 'ifc';
    // On tablet, split mode shows only the viewer (no side-by-side editor)
    const showEditor = viewMode === '3d';

    return (
      <div style={{
        display: 'flex', flexDirection: 'column',
        height: '100vh', overflow: 'hidden',
        fontFamily: appFont, background: 'var(--bg)', color: 'var(--text)',
      }}>
        <Toolbar />
        <div style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>
          <div style={{
            flex: 1, display: 'grid',
            gridTemplateColumns: showEditor ? '1fr 1fr' : '1fr',
            gridTemplateRows: '1fr 160px',
            gap: 1, background: 'var(--border)',
            overflow: 'hidden',
          }}>
            {showEditor && <CodeEditor />}
            {show3d && <ViewerPanel />}
            <div style={{ gridColumn: '1 / -1' }}>
              <DiagnosticsPanel />
            </div>
          </div>
          {chatSidebar}
        </div>
      </div>
    );
  }

  // --- Desktop layout (unchanged) ---
  const chatSidebar = chatOpen ? (
    <div style={{ width: 320, minWidth: 280, overflow: 'hidden' }}>
      <ChatPanel />
    </div>
  ) : null;

  if (viewMode === '2d') {
    return (
      <div style={{
        display: 'flex', flexDirection: 'column',
        height: '100vh', overflow: 'hidden',
        fontFamily: appFont, background: 'var(--bg)', color: 'var(--text)',
      }}>
        <Toolbar />
        <div style={{
          display: 'flex', alignItems: 'center', padding: '0 12px', gap: 8,
          height: 36, background: 'var(--surface)', borderBottom: '1px solid var(--border)',
        }}>
          <ToolSelector />
        </div>
        <div style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>
          <div style={{
            flex: 1, display: 'grid',
            gridTemplateColumns: '240px 1fr 280px',
            gap: 1, background: 'var(--border)',
            overflow: 'hidden',
          }}>
            <ElementTree />
            <FloorplanPanel />
            <div style={{ background: 'var(--surface)', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
              <PropertiesPanel />
              <BmlEditor />
            </div>
          </div>
          {chatSidebar}
        </div>
      </div>
    );
  }

  const show3d = viewMode === '3d' || viewMode === 'split' || viewMode === 'ifc';
  const showEditor = viewMode === '3d' || viewMode === 'split';

  return (
    <div style={{
      display: 'flex', flexDirection: 'column',
      height: '100vh', overflow: 'hidden',
      fontFamily: appFont, background: 'var(--bg)', color: 'var(--text)',
    }}>
      <Toolbar />
      <div style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>
        <div style={{
          flex: 1, display: 'grid',
          gridTemplateColumns: showEditor ? '1fr 1fr' : '1fr',
          gridTemplateRows: '1fr 200px',
          gap: 1, background: 'var(--border)',
          overflow: 'hidden',
        }}>
          {showEditor && <CodeEditor />}
          {show3d && <ViewerPanel />}
          <div style={{ gridColumn: '1 / -1' }}>
            <DiagnosticsPanel />
          </div>
        </div>
        {chatSidebar}
      </div>
    </div>
  );
}
