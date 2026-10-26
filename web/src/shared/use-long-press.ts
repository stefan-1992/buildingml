import { useRef, useCallback } from 'react';

const LONG_PRESS_MS = 500;
const MOVE_THRESHOLD = 10;

interface LongPressOptions {
  onLongPress: (x: number, y: number) => void;
  delay?: number;
}

/**
 * Returns touch event handlers that detect long-press gestures.
 * Fires `onLongPress(clientX, clientY)` after holding for `delay` ms
 * without moving beyond `MOVE_THRESHOLD` pixels.
 */
export function useLongPress({ onLongPress, delay = LONG_PRESS_MS }: LongPressOptions) {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const startPos = useRef<{ x: number; y: number } | null>(null);
  const firedRef = useRef(false);

  const clear = useCallback(() => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    startPos.current = null;
  }, []);

  const onTouchStart = useCallback((e: React.TouchEvent) => {
    if (e.touches.length !== 1) {
      clear();
      return;
    }
    const { clientX, clientY } = e.touches[0];
    startPos.current = { x: clientX, y: clientY };
    firedRef.current = false;

    timerRef.current = setTimeout(() => {
      firedRef.current = true;
      onLongPress(clientX, clientY);
      timerRef.current = null;
    }, delay);
  }, [onLongPress, delay, clear]);

  const onTouchMove = useCallback((e: React.TouchEvent) => {
    if (!startPos.current || e.touches.length !== 1) {
      clear();
      return;
    }
    const { clientX, clientY } = e.touches[0];
    const dx = clientX - startPos.current.x;
    const dy = clientY - startPos.current.y;
    if (Math.hypot(dx, dy) > MOVE_THRESHOLD) {
      clear();
    }
  }, [clear]);

  const onTouchEnd = useCallback(() => {
    clear();
  }, [clear]);

  return {
    onTouchStart,
    onTouchMove,
    onTouchEnd,
    /** True if the long-press callback was just fired (use to suppress tap). */
    didFire: () => firedRef.current,
  };
}
