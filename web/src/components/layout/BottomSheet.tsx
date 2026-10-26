import { useRef, useCallback, useState, useEffect, type ReactNode } from 'react';

export type SheetState = 'collapsed' | 'half' | 'full';

interface BottomSheetProps {
  children: ReactNode;
  title: string;
  state: SheetState;
  onStateChange: (state: SheetState) => void;
  /** Optional override for peek (collapsed) height. Default 56px. */
  peekHeight?: number;
}

const PEEK_HEIGHT = 56;
const DRAG_THRESHOLD = 20;

/**
 * Draggable bottom sheet with three states: collapsed (peek), half, full.
 * User can drag the handle to transition between states, or tap the handle to toggle.
 */
export function BottomSheet({
  children,
  title,
  state,
  onStateChange,
  peekHeight = PEEK_HEIGHT,
}: BottomSheetProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const dragStartY = useRef<number | null>(null);
  const dragStartState = useRef<SheetState>(state);
  const [dragOffset, setDragOffset] = useState(0);
  const [containerHeight, setContainerHeight] = useState(0);

  // Measure container parent height
  useEffect(() => {
    const measure = () => {
      if (containerRef.current?.parentElement) {
        setContainerHeight(containerRef.current.parentElement.clientHeight);
      }
    };
    measure();
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, []);

  const getHeightForState = useCallback((s: SheetState): number => {
    if (!containerHeight) return peekHeight;
    switch (s) {
      case 'collapsed': return peekHeight;
      case 'half': return Math.round(containerHeight * 0.5);
      case 'full': return Math.round(containerHeight * 0.92);
    }
  }, [containerHeight, peekHeight]);

  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    if (e.touches.length !== 1) return;
    dragStartY.current = e.touches[0].clientY;
    dragStartState.current = state;
    setDragOffset(0);
  }, [state]);

  const handleTouchMove = useCallback((e: React.TouchEvent) => {
    if (dragStartY.current === null || e.touches.length !== 1) return;
    e.preventDefault();
    const dy = dragStartY.current - e.touches[0].clientY;
    setDragOffset(dy);
  }, []);

  const handleTouchEnd = useCallback(() => {
    if (dragStartY.current === null) return;
    const dy = dragOffset;
    dragStartY.current = null;

    if (Math.abs(dy) < DRAG_THRESHOLD) {
      // Tap/small movement — toggle between collapsed and half
      setDragOffset(0);
      if (state === 'collapsed') onStateChange('half');
      else if (state === 'half') onStateChange('collapsed');
      else onStateChange('half');
      return;
    }

    // Determine target state from drag direction and current state
    setDragOffset(0);
    if (dy > 0) {
      // Dragged up
      if (state === 'collapsed') onStateChange('half');
      else if (state === 'half') onStateChange('full');
    } else {
      // Dragged down
      if (state === 'full') onStateChange('half');
      else if (state === 'half') onStateChange('collapsed');
    }
  }, [dragOffset, state, onStateChange]);

  const baseHeight = getHeightForState(state);
  const currentHeight = Math.max(peekHeight, Math.min(
    containerHeight * 0.92,
    baseHeight + dragOffset,
  ));

  return (
    <div
      ref={containerRef}
      style={{
        position: 'absolute',
        bottom: 0,
        left: 0,
        right: 0,
        height: currentHeight,
        background: 'var(--surface)',
        borderTop: '1px solid var(--border)',
        borderRadius: '12px 12px 0 0',
        zIndex: 50,
        display: 'flex',
        flexDirection: 'column',
        transition: dragStartY.current !== null ? 'none' : 'height 0.25s ease-out',
        boxShadow: '0 -4px 20px rgba(0,0,0,0.3)',
        overflow: 'hidden',
      }}
    >
      {/* Drag handle */}
      <div
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
        onClick={() => {
          if (state === 'collapsed') onStateChange('half');
          else if (state === 'half') onStateChange('collapsed');
          else onStateChange('half');
        }}
        style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          padding: '8px 16px',
          cursor: 'grab',
          touchAction: 'none',
          userSelect: 'none',
          WebkitUserSelect: 'none',
          flexShrink: 0,
        }}
      >
        {/* Visual drag indicator */}
        <div style={{
          width: 36,
          height: 4,
          borderRadius: 2,
          background: 'var(--text-dim)',
          opacity: 0.5,
          marginBottom: 6,
        }} />
        <div style={{
          fontSize: 12,
          fontWeight: 600,
          color: 'var(--text)',
          textTransform: 'uppercase',
          letterSpacing: 0.5,
        }}>
          {title}
        </div>
      </div>

      {/* Content area */}
      <div style={{
        flex: 1,
        overflow: state === 'collapsed' ? 'hidden' : 'auto',
        opacity: state === 'collapsed' ? 0 : 1,
        transition: 'opacity 0.15s ease',
      }}>
        {children}
      </div>
    </div>
  );
}
