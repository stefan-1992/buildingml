import { describe, it, expect } from 'vitest';
import {
  DEFAULT_SECTION_CUT,
  computeSectionCutBounds,
  computeLateralCutBounds,
  cutFraction,
  applyCutFraction,
  type SectionCutState,
} from '../section-cut.ts';
import type { SceneGraph } from '../../types.ts';

function vertex(x: number, y: number, z: number) {
  return { position: [x, y, z] as [number, number, number], normal: [0, 0, 1] as [number, number, number] };
}

function sceneOf(points: Array<[number, number, number]>): SceneGraph {
  return {
    nodes: [
      {
        id: 'n1',
        element_type: 'Wall',
        label: 'n1',
        mesh: {
          vertices: points.map((p) => vertex(p[0], p[1], p[2])),
          indices: [],
          color: [1, 1, 1, 1],
        },
        children: [],
      },
    ],
  } as unknown as SceneGraph;
}

function cut(min: number, max: number, value: number): SectionCutState {
  return { enabled: true, min, max, value, step: (max - min) / 200 };
}

describe('section cut defaults', () => {
  it('starts disabled so a fresh viewer shows the whole model', () => {
    expect(DEFAULT_SECTION_CUT.enabled).toBe(false);
  });

  it('stays disabled when bounds are computed from a scene', () => {
    const s = sceneOf([[0, 0, 0], [1, 2, 8]]);
    expect(computeSectionCutBounds(s).enabled).toBe(false);
    expect(computeLateralCutBounds(s).enabled).toBe(false);
  });

  it('spans the full extent of the axis it measures', () => {
    const s = sceneOf([[0, -4, -1], [1, 6, 19]]);
    const z = computeSectionCutBounds(s);
    expect([z.min, z.max, z.value]).toEqual([-1, 19, 19]);
    const y = computeLateralCutBounds(s);
    expect([y.min, y.max, y.value]).toEqual([-4, 6, 6]);
  });
});

describe('cutFraction', () => {
  it('reports 1 at the no-clip end and 0 at the bottom', () => {
    expect(cutFraction(cut(-1, 19, 19))).toBe(1);
    expect(cutFraction(cut(-1, 19, -1))).toBe(0);
  });

  it('reports the midpoint as one half', () => {
    expect(cutFraction(cut(0, 10, 5))).toBeCloseTo(0.5);
  });

  it('treats a zero-extent model as no-clip rather than dividing by zero', () => {
    expect(cutFraction(cut(3, 3, 3))).toBe(1);
  });
});

describe('applyCutFraction', () => {
  it('snaps the top of the range exactly to max, so no-clip survives a reload', () => {
    const c = cut(-1.02, 19.1, 0);
    applyCutFraction(c, 1);
    expect(c.value).toBe(19.1);
  });

  it('keeps a mid-model cut proportional when the model changes size', () => {
    const small = cut(0, 10, 5);
    const big = cut(0, 40, 0);
    applyCutFraction(big, cutFraction(small));
    expect(big.value).toBeCloseTo(20);
  });

  it('never restores a cut inside a taller model from an absolute value', () => {
    // The regression: 8 m saved from a 10 m model, reopened on a 19.1 m one.
    const remembered = cutFraction(cut(0, 10, 10)); // user left it at no-clip
    const taller = cut(-1.02, 19.1, 0);
    applyCutFraction(taller, remembered);
    expect(taller.value).toBe(taller.max);
  });

  it('does not leave the range for a fraction at either end', () => {
    const c = cut(2, 6, 2);
    applyCutFraction(c, 0);
    expect(c.value).toBe(2);
    applyCutFraction(c, 0.5);
    expect(c.value).toBeCloseTo(4);
  });
});
