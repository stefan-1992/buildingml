import { describe, it, expect } from 'vitest';

// Test the breakpoint classification logic directly (no DOM needed)
function getBreakpoint(width: number): 'phone' | 'tablet' | 'desktop' {
  if (width <= 767) return 'phone';
  if (width <= 1024) return 'tablet';
  return 'desktop';
}

describe('breakpoint classification', () => {
  it('classifies phone breakpoint (<768)', () => {
    expect(getBreakpoint(375)).toBe('phone');
    expect(getBreakpoint(320)).toBe('phone');
    expect(getBreakpoint(767)).toBe('phone');
  });

  it('classifies tablet breakpoint (768-1024)', () => {
    expect(getBreakpoint(768)).toBe('tablet');
    expect(getBreakpoint(800)).toBe('tablet');
    expect(getBreakpoint(1024)).toBe('tablet');
  });

  it('classifies desktop breakpoint (>1024)', () => {
    expect(getBreakpoint(1025)).toBe('desktop');
    expect(getBreakpoint(1440)).toBe('desktop');
    expect(getBreakpoint(1920)).toBe('desktop');
  });

  it('boundary: 767 is phone, 768 is tablet', () => {
    expect(getBreakpoint(767)).toBe('phone');
    expect(getBreakpoint(768)).toBe('tablet');
  });

  it('boundary: 1024 is tablet, 1025 is desktop', () => {
    expect(getBreakpoint(1024)).toBe('tablet');
    expect(getBreakpoint(1025)).toBe('desktop');
  });
});
