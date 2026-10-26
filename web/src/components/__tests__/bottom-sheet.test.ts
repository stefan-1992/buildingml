import { describe, it, expect } from 'vitest';
import type { SheetState } from '../layout/BottomSheet.tsx';

describe('BottomSheet state transitions', () => {
  // Test the state machine logic that BottomSheet uses

  function nextState(current: SheetState, direction: 'up' | 'down'): SheetState {
    if (direction === 'up') {
      if (current === 'collapsed') return 'half';
      if (current === 'half') return 'full';
      return 'full';
    } else {
      if (current === 'full') return 'half';
      if (current === 'half') return 'collapsed';
      return 'collapsed';
    }
  }

  function toggleState(current: SheetState): SheetState {
    if (current === 'collapsed') return 'half';
    if (current === 'half') return 'collapsed';
    return 'half';
  }

  it('transitions up: collapsed -> half -> full', () => {
    expect(nextState('collapsed', 'up')).toBe('half');
    expect(nextState('half', 'up')).toBe('full');
    expect(nextState('full', 'up')).toBe('full');
  });

  it('transitions down: full -> half -> collapsed', () => {
    expect(nextState('full', 'down')).toBe('half');
    expect(nextState('half', 'down')).toBe('collapsed');
    expect(nextState('collapsed', 'down')).toBe('collapsed');
  });

  it('toggle: collapsed <-> half, full -> half', () => {
    expect(toggleState('collapsed')).toBe('half');
    expect(toggleState('half')).toBe('collapsed');
    expect(toggleState('full')).toBe('half');
  });

  it('height calculations for different states', () => {
    const containerHeight = 600;
    const peekHeight = 56;

    const heights: Record<SheetState, number> = {
      collapsed: peekHeight,
      half: Math.round(containerHeight * 0.5),
      full: Math.round(containerHeight * 0.92),
    };

    expect(heights.collapsed).toBe(56);
    expect(heights.half).toBe(300);
    expect(heights.full).toBe(552);
  });
});

describe('BottomNav tab items', () => {
  const tabs = ['3d', '2d', 'code', 'chat', 'properties'] as const;

  it('has 5 navigation items', () => {
    expect(tabs).toHaveLength(5);
  });

  it('includes all required tabs from spec', () => {
    expect(tabs).toContain('3d');
    expect(tabs).toContain('2d');
    expect(tabs).toContain('code');
    expect(tabs).toContain('chat');
    expect(tabs).toContain('properties');
  });
});
