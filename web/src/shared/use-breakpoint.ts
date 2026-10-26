import { useState, useEffect } from 'react';

export type Breakpoint = 'phone' | 'tablet' | 'desktop';

const PHONE_MAX = 767;
const TABLET_MAX = 1024;

function getBreakpoint(): Breakpoint {
  if (typeof window === 'undefined') return 'desktop';
  const w = window.innerWidth;
  if (w <= PHONE_MAX) return 'phone';
  if (w <= TABLET_MAX) return 'tablet';
  return 'desktop';
}

/** Returns the current responsive breakpoint: phone (<768), tablet (768-1024), desktop (>1024). */
export function useBreakpoint(): Breakpoint {
  const [bp, setBp] = useState(getBreakpoint);

  useEffect(() => {
    const handler = () => setBp(getBreakpoint());
    window.addEventListener('resize', handler);
    return () => window.removeEventListener('resize', handler);
  }, []);

  return bp;
}
