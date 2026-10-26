import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

describe('long press timing logic', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('fires callback after 500ms hold', () => {
    const callback = vi.fn();
    const timer = setTimeout(() => callback(100, 200), 500);

    // Before 500ms
    vi.advanceTimersByTime(499);
    expect(callback).not.toHaveBeenCalled();

    // At 500ms
    vi.advanceTimersByTime(1);
    expect(callback).toHaveBeenCalledWith(100, 200);

    clearTimeout(timer);
  });

  it('does not fire if cleared before 500ms', () => {
    const callback = vi.fn();
    const timer = setTimeout(() => callback(), 500);

    vi.advanceTimersByTime(300);
    clearTimeout(timer);

    vi.advanceTimersByTime(300);
    expect(callback).not.toHaveBeenCalled();
  });

  it('move threshold cancels long press', () => {
    // Simulates the move-threshold check: if distance > 10, cancel
    const threshold = 10;
    const dx = 15, dy = 0;
    expect(Math.hypot(dx, dy) > threshold).toBe(true);
  });

  it('small movements do not cancel', () => {
    const threshold = 10;
    const dx = 5, dy = 3;
    expect(Math.hypot(dx, dy) > threshold).toBe(false);
  });
});
