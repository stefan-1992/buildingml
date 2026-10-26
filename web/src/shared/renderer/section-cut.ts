import type { SceneGraph, SceneNode } from '../types.ts';
import { forEachPosition } from './scene-loader.ts';

export interface SectionCutState {
  enabled: boolean;
  value: number;
  min: number;
  max: number;
  step: number;
}

/**
 * Cuts start switched off. A cut parked at its maximum shows the whole model
 * too, but only until the next model loads with different bounds - off is the
 * only state that cannot come back slicing a building in half.
 */
export const DEFAULT_SECTION_CUT: SectionCutState = {
  enabled: false,
  value: 10,
  min: 0,
  max: 10,
  step: 0.05,
};

function boundsFromAxis(sceneData: SceneGraph, axis: 0 | 1 | 2): SectionCutState {
  let lo = Infinity;
  let hi = -Infinity;

  function scanNode(node: SceneNode): void {
    forEachPosition(node.mesh, (x, y, z) => {
      const c = axis === 0 ? x : axis === 1 ? y : z;
      lo = Math.min(lo, c);
      hi = Math.max(hi, c);
    });
    if (node.children) node.children.forEach(scanNode);
  }
  sceneData.nodes.forEach(scanNode);

  if (lo === Infinity) return { ...DEFAULT_SECTION_CUT };

  const range = hi - lo;
  return {
    enabled: false,
    value: hi,
    min: lo,
    max: hi,
    step: range > 0 ? range / 200 : 0.05,
  };
}

/** Compute Z-axis bounds from scene graph vertices (elevation / plan cut). */
export function computeSectionCutBounds(sceneData: SceneGraph): SectionCutState {
  return boundsFromAxis(sceneData, 2);
}

/** Compute Y-axis bounds from scene graph vertices (lateral / section cut). */
export function computeLateralCutBounds(sceneData: SceneGraph): SectionCutState {
  return boundsFromAxis(sceneData, 1);
}

/** Where the cut sits in its own range, 0 at the bottom and 1 at no-clip. */
export function cutFraction(cut: SectionCutState): number {
  const range = cut.max - cut.min;
  return range > 0 ? (cut.value - cut.min) / range : 1;
}

/**
 * Place a cut at a remembered fraction of the *current* bounds.
 *
 * Cut positions are carried across models as a fraction rather than as metres,
 * because an absolute height restored into a model of a different size lands
 * somewhere arbitrary - which is how a fresh preview ends up half-sliced. A
 * fraction at the top snaps exactly to max so no-clip stays no-clip.
 */
export function applyCutFraction(cut: SectionCutState, fraction: number): void {
  cut.value = fraction >= 0.999 ? cut.max : cut.min + fraction * (cut.max - cut.min);
}
