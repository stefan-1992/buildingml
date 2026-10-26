import { describe, it, expect, vi, beforeEach, beforeAll } from 'vitest';
import type { SceneGraph } from '../../types.ts';

// Shim browser globals for Node.js test environment
beforeAll(() => {
  if (typeof globalThis.requestAnimationFrame === 'undefined') {
    (globalThis as any).requestAnimationFrame = (cb: FrameRequestCallback) => setTimeout(cb, 0) as unknown as number;
  }
  if (typeof globalThis.cancelAnimationFrame === 'undefined') {
    (globalThis as any).cancelAnimationFrame = (id: number) => clearTimeout(id);
  }
});

// vi.mock is hoisted — all mock state must be inside the factory.
// We use globalThis to share mock references with tests.
vi.mock('three', () => {
  const addFn = vi.fn();
  const clearFn = vi.fn();
  const rendererDisposeFn = vi.fn();
  const renderFn = vi.fn();
  const materialDisposeFn = vi.fn();

  (globalThis as any).__threeMocks = { addFn, clearFn, rendererDisposeFn, renderFn, materialDisposeFn };

  // Use function() constructors instead of arrow functions so `new` works.
  function MockWebGLRenderer() {
    return {
      setClearColor: vi.fn(),
      setPixelRatio: vi.fn(),
      setSize: vi.fn(),
      render: renderFn,
      dispose: rendererDisposeFn,
      sortObjects: true,
    };
  }

  function MockScene() { return { add: vi.fn(), remove: vi.fn() }; }
  function MockPerspectiveCamera() {
    return {
      aspect: 1, position: { set: vi.fn() }, up: { set: vi.fn() },
      lookAt: vi.fn(), updateProjectionMatrix: vi.fn(), updateMatrixWorld: vi.fn(),
    };
  }
  function MockGroup() {
    const children: any[] = [];
    return {
      add: (...args: any[]) => { children.push(...args); addFn(...args); },
      clear: () => { children.length = 0; clearFn(); },
      children,
    };
  }
  function MockBufferGeometry() { return { setAttribute: vi.fn(), setIndex: vi.fn(), dispose: vi.fn(), computeBoundingSphere: vi.fn() }; }
  function MockBufferAttribute(_array: any, _itemSize: number) { return { array: _array, itemSize: _itemSize }; }
  function MockShaderMaterial() {
    return {
      uniforms: {
        uColor: { value: {} },
        uLightDir: { value: {} },
        uCameraPos: { value: { set: vi.fn() } },
        uClipEnabled: { value: false },
        uClipZ: { value: 10.0 },
        uClipYEnabled: { value: false },
        uClipY: { value: 10.0 },
        uHighlight: { value: 0.0 },
        uHighlightColor: { value: {} },
      },
      wireframe: false,
      dispose: materialDisposeFn,
    };
  }
  function MockMesh() { return { name: '', userData: {}, renderOrder: 0, visible: true }; }
  function MockVector2() { return { x: 0, y: 0, set: vi.fn() }; }
  function MockVector3() {
    return { x: 0, y: 0, z: 0, set: vi.fn(), copy: vi.fn(), normalize() { return this; } };
  }
  function MockVector4() { return { x: 0, y: 0, z: 0, w: 0 }; }
  function MockColor() { return { r: 0, g: 0, b: 0 }; }
  function MockRaycaster() {
    return {
      setFromCamera: vi.fn(),
      intersectObjects: vi.fn().mockReturnValue([]),
    };
  }

  return {
    WebGLRenderer: MockWebGLRenderer,
    Scene: MockScene,
    PerspectiveCamera: MockPerspectiveCamera,
    Group: MockGroup,
    BufferGeometry: MockBufferGeometry,
    BufferAttribute: MockBufferAttribute,
    ShaderMaterial: MockShaderMaterial,
    Mesh: MockMesh,
    Vector2: MockVector2,
    Vector3: MockVector3,
    Vector4: MockVector4,
    Color: MockColor,
    Raycaster: MockRaycaster,
    DoubleSide: 2,
  };
});

// Mock TransformControls
vi.mock('three/addons/controls/TransformControls.js', () => {
  function MockTransformControls() {
    const listeners: Record<string, Function[]> = {};
    return {
      setMode: vi.fn(),
      setSpace: vi.fn(),
      setSize: vi.fn(),
      attach: vi.fn(),
      detach: vi.fn(),
      dispose: vi.fn(),
      getHelper: vi.fn().mockReturnValue({ type: 'TransformControlsGizmo' }),
      addEventListener: vi.fn((event: string, cb: Function) => {
        listeners[event] = listeners[event] || [];
        listeners[event].push(cb);
      }),
      removeEventListener: vi.fn(),
    };
  }
  return { TransformControls: MockTransformControls };
});

import { ThreeJSRenderer } from '../threejs-renderer.ts';

function getMocks() {
  return (globalThis as any).__threeMocks as {
    addFn: ReturnType<typeof vi.fn>;
    clearFn: ReturnType<typeof vi.fn>;
    rendererDisposeFn: ReturnType<typeof vi.fn>;
    renderFn: ReturnType<typeof vi.fn>;
    materialDisposeFn: ReturnType<typeof vi.fn>;
  };
}

function createTestScene(): SceneGraph {
  return {
    nodes: [
      {
        id: 'wall-1',
        element_type: 'Wall',
        label: 'Wall W-01',
        mesh: {
          vertices: [
            { position: [0, 0, 0], normal: [0, 0, 1] },
            { position: [6, 0, 0], normal: [0, 0, 1] },
            { position: [6, 0, 2.8], normal: [0, 0, 1] },
            { position: [0, 0, 2.8], normal: [0, 0, 1] },
          ],
          indices: [0, 1, 2, 0, 2, 3],
          color: [0.85, 0.82, 0.78, 1.0],
        },
        children: [],
        properties: [['from', '(0, 0) to (6, 0)'], ['height', '2.8'], ['thickness', '0.24']],
      },
    ],
  };
}

function createTransparentScene(): SceneGraph {
  return {
    nodes: [
      {
        id: 'window-1',
        element_type: 'Window',
        label: 'Window WIN-01',
        mesh: {
          vertices: [
            { position: [1, 0, 0.8], normal: [0, 0, 1] },
            { position: [2, 0, 0.8], normal: [0, 0, 1] },
            { position: [2, 0, 2.2], normal: [0, 0, 1] },
          ],
          indices: [0, 1, 2],
          color: [0.4, 0.6, 0.8, 0.4],
        },
        children: [],
      },
    ],
  };
}

function createCanvas(): HTMLCanvasElement {
  return {
    getContext: vi.fn().mockReturnValue({}),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }),
    parentElement: {
      getBoundingClientRect: () => ({ width: 800, height: 600 }),
    },
    width: 800,
    height: 600,
    style: { width: '', height: '' },
  } as unknown as HTMLCanvasElement;
}

describe('ThreeJSRenderer', () => {
  let canvas: HTMLCanvasElement;

  beforeEach(() => {
    canvas = createCanvas();
    const mocks = getMocks();
    mocks.addFn.mockClear();
    mocks.clearFn.mockClear();
    mocks.rendererDisposeFn.mockClear();
    mocks.renderFn.mockClear();
    mocks.materialDisposeFn.mockClear();
  });

  it('constructs without error', () => {
    const renderer = new ThreeJSRenderer(canvas);
    expect(renderer).toBeDefined();
    expect(renderer.camera).toBeDefined();
    expect(renderer.wireframe).toBe(false);
    renderer.dispose();
  });

  it('has default camera state', () => {
    const renderer = new ThreeJSRenderer(canvas);
    expect(renderer.camera.distance).toBe(15);
    expect(renderer.camera.rotX).toBe(0.5);
    expect(renderer.camera.rotY).toBe(0.8);
    expect(renderer.camera.target).toEqual([3, 2.5, 1.4]);
    renderer.dispose();
  });

  it('loads a scene graph', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    expect(getMocks().addFn).toHaveBeenCalled();
    renderer.dispose();
  });

  it('fits camera to scene after loading', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());

    expect(renderer.camera.target[0]).toBeCloseTo(3);
    expect(renderer.camera.target[1]).toBeCloseTo(0);
    expect(renderer.camera.target[2]).toBeCloseTo(1.4);
    renderer.dispose();
  });

  it('computes section cut bounds from scene', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());

    expect(renderer.sectionCut.min).toBeCloseTo(0);
    expect(renderer.sectionCut.max).toBeCloseTo(2.8);
    expect(renderer.sectionCut.value).toBeCloseTo(2.8);
    renderer.dispose();
  });

  it('computes lateral cut bounds from scene', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());

    expect(renderer.lateralCut.min).toBeCloseTo(0);
    expect(renderer.lateralCut.max).toBeCloseTo(0);
    renderer.dispose();
  });

  it('clears previous meshes when loading new scene', () => {
    const renderer = new ThreeJSRenderer(canvas);
    const mocks = getMocks();

    renderer.loadScene(createTestScene());
    expect(mocks.addFn).toHaveBeenCalled();

    mocks.addFn.mockClear();
    mocks.clearFn.mockClear();

    renderer.loadScene(createTestScene());
    expect(mocks.clearFn).toHaveBeenCalled();
    expect(mocks.addFn).toHaveBeenCalled();
    renderer.dispose();
  });

  it('handles transparent materials', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTransparentScene());
    expect(getMocks().addFn).toHaveBeenCalled();
    renderer.dispose();
  });

  it('handles empty scene', () => {
    const renderer = new ThreeJSRenderer(canvas);
    const mocks = getMocks();
    mocks.addFn.mockClear();

    renderer.loadScene({ nodes: [] });
    expect(mocks.addFn).not.toHaveBeenCalled();
    renderer.dispose();
  });

  it('handles children nodes recursively', () => {
    const renderer = new ThreeJSRenderer(canvas);
    const mocks = getMocks();
    mocks.addFn.mockClear();

    const scene: SceneGraph = {
      nodes: [
        {
          id: 'parent',
          element_type: 'Wall',
          label: 'Parent Wall',
          mesh: {
            vertices: [
              { position: [0, 0, 0], normal: [0, 0, 1] },
              { position: [1, 0, 0], normal: [0, 0, 1] },
              { position: [0, 1, 0], normal: [0, 0, 1] },
            ],
            indices: [0, 1, 2],
            color: [0.8, 0.8, 0.8, 1.0],
          },
          children: [
            {
              id: 'child-door',
              element_type: 'Door',
              label: 'Door D-01',
              mesh: {
                vertices: [
                  { position: [0.2, 0, 0], normal: [0, 0, 1] },
                  { position: [0.8, 0, 0], normal: [0, 0, 1] },
                  { position: [0.8, 0, 2.1], normal: [0, 0, 1] },
                ],
                indices: [0, 1, 2],
                color: [0.5, 0.3, 0.1, 1.0],
              },
              children: [],
            },
          ],
        },
      ],
    };
    renderer.loadScene(scene);
    expect(mocks.addFn).toHaveBeenCalledTimes(2);
    renderer.dispose();
  });

  it('start and stop control the render loop', () => {
    const renderer = new ThreeJSRenderer(canvas);
    const rafSpy = vi.spyOn(globalThis, 'requestAnimationFrame').mockReturnValue(1);
    const cafSpy = vi.spyOn(globalThis, 'cancelAnimationFrame').mockImplementation(() => {});

    renderer.start();
    expect(rafSpy).toHaveBeenCalled();

    renderer.stop();
    expect(cafSpy).toHaveBeenCalledWith(1);

    rafSpy.mockRestore();
    cafSpy.mockRestore();
    renderer.dispose();
  });

  it('dispose cleans up resources', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    renderer.start();
    renderer.dispose();

    const mocks = getMocks();
    expect(mocks.rendererDisposeFn).toHaveBeenCalled();
    expect(mocks.clearFn).toHaveBeenCalled();
  });

  it('wireframe property can be toggled', () => {
    const renderer = new ThreeJSRenderer(canvas);
    expect(renderer.wireframe).toBe(false);
    renderer.wireframe = true;
    expect(renderer.wireframe).toBe(true);
    renderer.dispose();
  });

  it('section cut state can be modified', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.sectionCut = { enabled: true, value: 5, min: 0, max: 10, step: 0.1 };
    expect(renderer.sectionCut.value).toBe(5);
    expect(renderer.sectionCut.enabled).toBe(true);
    renderer.dispose();
  });

  it('lateral cut state can be modified', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.lateralCut = { enabled: false, value: 3, min: 0, max: 8, step: 0.05 };
    expect(renderer.lateralCut.value).toBe(3);
    expect(renderer.lateralCut.enabled).toBe(false);
    renderer.dispose();
  });

  // --- New tests for 3D interactive editing ---

  it('highlightedId defaults to null', () => {
    const renderer = new ThreeJSRenderer(canvas);
    expect(renderer.highlightedId).toBeNull();
    renderer.dispose();
  });

  it('pick returns null when nothing is hit', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    const result = renderer.pick(400, 300);
    expect(result).toBeNull();
    renderer.dispose();
  });

  it('stores properties in mesh userData', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    // The mock meshGroup children should have userData with properties
    const meshGroup = (renderer as any).meshGroup;
    expect(meshGroup.children.length).toBeGreaterThan(0);
    const firstMesh = meshGroup.children[0];
    expect(firstMesh.userData.properties).toBeDefined();
    expect(firstMesh.userData.properties.length).toBe(3);
    expect(firstMesh.userData.properties[0][0]).toBe('from');
    renderer.dispose();
  });

  it('onTransform defaults to null', () => {
    const renderer = new ThreeJSRenderer(canvas);
    expect(renderer.onTransform).toBeNull();
    renderer.dispose();
  });

  it('isDraggingGizmo defaults to false', () => {
    const renderer = new ThreeJSRenderer(canvas);
    expect(renderer.isDraggingGizmo).toBe(false);
    renderer.dispose();
  });

  it('attachGizmo(null) does not throw', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    expect(() => renderer.attachGizmo(null)).not.toThrow();
    renderer.dispose();
  });

  it('loadScene resets highlightedId', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.highlightedId = 'some-id';
    renderer.loadScene(createTestScene());
    expect(renderer.highlightedId).toBeNull();
    renderer.dispose();
  });

  it('hiddenIds defaults to empty and leaves meshes visible', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    expect(renderer.hiddenIds.size).toBe(0);
    expect(getMocks().addFn.mock.calls[0][0].visible).toBe(true);
    renderer.dispose();
  });

  it('hiddenIds hides the matching mesh and unhides on clear', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    const mesh = getMocks().addFn.mock.calls[0][0];

    renderer.hiddenIds = new Set(['wall-1']);
    expect(mesh.visible).toBe(false);

    renderer.hiddenIds = new Set();
    expect(mesh.visible).toBe(true);
    renderer.dispose();
  });

  it('re-applies hiddenIds across a scene reload', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    renderer.hiddenIds = new Set(['wall-1']);

    renderer.loadScene(createTestScene());
    const reloaded = getMocks().addFn.mock.calls.at(-1)![0];
    expect(reloaded.visible).toBe(false);
    renderer.dispose();
  });

  it('listElements returns one entry per element with its properties', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    const elements = renderer.listElements();
    expect(elements).toHaveLength(1);
    expect(elements[0].id).toBe('wall-1');
    expect(elements[0].elementType).toBe('Wall');
    expect(elements[0].properties).toContainEqual(['height', '2.8']);
    renderer.dispose();
  });

  it('child sub-meshes inherit the parent element properties', () => {
    const renderer = new ThreeJSRenderer(canvas);
    const scene = createTestScene();
    // A second material on the same element arrives as a child sharing its id.
    scene.nodes[0].children = [{
      ...scene.nodes[0],
      children: [],
      properties: [],
    }];
    renderer.loadScene(scene);

    const [parent, child] = getMocks().addFn.mock.calls.map((c: any[]) => c[0]);
    expect(child.userData.properties).toEqual(parent.userData.properties);
    // Both parts are one element, so the list must not double-count it.
    expect(renderer.listElements()).toHaveLength(1);
    renderer.dispose();
  });

  it('pick skips hidden geometry the raycaster still reports', () => {
    const renderer = new ThreeJSRenderer(canvas);
    renderer.loadScene(createTestScene());
    const mesh = getMocks().addFn.mock.calls[0][0];
    (renderer as any).raycaster.intersectObjects.mockReturnValue([{ object: mesh }]);

    expect(renderer.pick(10, 10)?.id).toBe('wall-1');

    renderer.hiddenIds = new Set(['wall-1']);
    expect(renderer.pick(10, 10)).toBeNull();
    renderer.dispose();
  });
});
