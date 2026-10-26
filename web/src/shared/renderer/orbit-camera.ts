/** Orbit camera with rotate, pan, and zoom. Framework-agnostic.
 *  Supports both mouse and touch (single-finger rotate, two-finger pan + pinch zoom). */

export interface CameraState {
  distance: number;
  rotX: number; // pitch
  rotY: number; // yaw
  target: [number, number, number];
}

export const DEFAULT_CAMERA: CameraState = {
  distance: 15,
  rotX: 0.5,
  rotY: 0.8,
  target: [3, 2.5, 1.4],
};

export function getCameraPosition(cam: CameraState): [number, number, number] {
  const x = cam.target[0] + cam.distance * Math.cos(cam.rotX) * Math.sin(cam.rotY);
  const y = cam.target[1] + cam.distance * Math.cos(cam.rotX) * Math.cos(cam.rotY);
  const z = cam.target[2] + cam.distance * Math.sin(cam.rotX);
  return [x, y, z];
}

export class OrbitCameraController {
  camera: CameraState;

  /** Set to true to suppress orbit camera (e.g. when gizmo is being dragged). */
  suppressDrag = false;

  /** Called on single-finger tap (quick touch without movement). */
  onTap: ((clientX: number, clientY: number) => void) | null = null;

  /** Called on long press (500ms hold without movement). */
  onLongPress: ((clientX: number, clientY: number) => void) | null = null;

  private isDragging = false;
  private isPanning = false;
  private lastMouse: [number, number] = [0, 0];
  private canvas: HTMLCanvasElement;

  // Touch state
  private lastTouchDist = 0;
  private lastTouchCenter: [number, number] = [0, 0];
  private touchCount = 0;
  private touchStartPos: [number, number] = [0, 0];
  private touchMoved = false;
  private longPressTimer: ReturnType<typeof setTimeout> | null = null;
  private longPressFired = false;

  // Bound handlers for cleanup
  private onMouseDown: (e: MouseEvent) => void;
  private onMouseMove: (e: MouseEvent) => void;
  private onMouseUp: () => void;
  private onWheel: (e: WheelEvent) => void;
  private onTouchStart: (e: TouchEvent) => void;
  private onTouchMove: (e: TouchEvent) => void;
  private onTouchEnd: (e: TouchEvent) => void;

  constructor(canvas: HTMLCanvasElement, initialCamera?: CameraState) {
    this.canvas = canvas;
    this.camera = initialCamera ? { ...initialCamera, target: [...initialCamera.target] } : { ...DEFAULT_CAMERA, target: [...DEFAULT_CAMERA.target] };

    // --- Mouse handlers ---
    this.onMouseDown = (e: MouseEvent) => {
      if (this.suppressDrag) return;
      this.isDragging = true;
      this.isPanning = e.shiftKey;
      this.lastMouse = [e.clientX, e.clientY];
      e.preventDefault();
    };

    this.onMouseMove = (e: MouseEvent) => {
      if (!this.isDragging || this.suppressDrag) return;
      const dx = e.clientX - this.lastMouse[0];
      const dy = e.clientY - this.lastMouse[1];
      this.lastMouse = [e.clientX, e.clientY];

      if (this.isPanning) {
        this.pan(dx, dy);
      } else {
        this.rotate(dx, dy);
      }
    };

    this.onMouseUp = () => {
      this.isDragging = false;
      this.isPanning = false;
    };

    this.onWheel = (e: WheelEvent) => {
      this.zoom(e.deltaY > 0 ? 1.1 : 0.9);
      e.preventDefault();
    };

    // --- Touch handlers ---
    this.onTouchStart = (e: TouchEvent) => {
      e.preventDefault();
      this.touchCount = e.touches.length;
      if (e.touches.length === 1) {
        // Single finger: defer isDragging until movement detected
        this.touchStartPos = [e.touches[0].clientX, e.touches[0].clientY];
        this.lastMouse = [e.touches[0].clientX, e.touches[0].clientY];
        this.touchMoved = false;
        this.longPressFired = false;
        // Start long-press timer (500ms)
        this.clearLongPressTimer();
        this.longPressTimer = setTimeout(() => {
          if (!this.touchMoved && this.onLongPress) {
            this.longPressFired = true;
            this.onLongPress(this.touchStartPos[0], this.touchStartPos[1]);
          }
        }, 500);
      } else if (e.touches.length === 2) {
        // Two fingers → pan + pinch
        this.isDragging = false;
        this.touchMoved = true; // suppress tap on multi-touch
        this.clearLongPressTimer();
        const t0 = e.touches[0];
        const t1 = e.touches[1];
        this.lastTouchDist = Math.hypot(t1.clientX - t0.clientX, t1.clientY - t0.clientY);
        this.lastTouchCenter = [(t0.clientX + t1.clientX) / 2, (t0.clientY + t1.clientY) / 2];
      }
    };

    this.onTouchMove = (e: TouchEvent) => {
      e.preventDefault();
      if (e.touches.length === 1 && this.touchCount === 1) {
        // Check if finger moved beyond threshold
        if (!this.touchMoved) {
          const dx = e.touches[0].clientX - this.touchStartPos[0];
          const dy = e.touches[0].clientY - this.touchStartPos[1];
          if (Math.abs(dx) > 8 || Math.abs(dy) > 8) {
            this.touchMoved = true;
            this.clearLongPressTimer();
            this.isDragging = true;
            this.isPanning = false;
            this.lastMouse = [e.touches[0].clientX, e.touches[0].clientY];
          }
          return; // Don't rotate yet while deciding
        }
        // Once confirmed as drag, rotate normally
        if (this.isDragging) {
          const dx = e.touches[0].clientX - this.lastMouse[0];
          const dy = e.touches[0].clientY - this.lastMouse[1];
          this.lastMouse = [e.touches[0].clientX, e.touches[0].clientY];
          this.rotate(dx, dy);
        }
      } else if (e.touches.length === 2) {
        const t0 = e.touches[0];
        const t1 = e.touches[1];
        const dist = Math.hypot(t1.clientX - t0.clientX, t1.clientY - t0.clientY);
        const center: [number, number] = [(t0.clientX + t1.clientX) / 2, (t0.clientY + t1.clientY) / 2];

        // Pinch zoom
        if (this.lastTouchDist > 0) {
          const scale = this.lastTouchDist / dist;
          this.zoom(scale);
        }

        // Two-finger pan
        const dx = center[0] - this.lastTouchCenter[0];
        const dy = center[1] - this.lastTouchCenter[1];
        if (Math.abs(dx) > 0.5 || Math.abs(dy) > 0.5) {
          this.pan(dx, dy);
        }

        this.lastTouchDist = dist;
        this.lastTouchCenter = center;
      }
    };

    this.onTouchEnd = (e: TouchEvent) => {
      this.clearLongPressTimer();
      this.touchCount = e.touches.length;
      if (e.touches.length === 0) {
        // If no significant movement and long press didn't fire, treat as tap
        if (!this.touchMoved && !this.longPressFired && this.onTap) {
          this.onTap(this.touchStartPos[0], this.touchStartPos[1]);
        }
        this.isDragging = false;
        this.isPanning = false;
        this.lastTouchDist = 0;
        this.touchMoved = false;
        this.longPressFired = false;
      } else if (e.touches.length === 1) {
        // Went from two fingers to one — restart single-finger mode
        this.isDragging = true;
        this.isPanning = false;
        this.lastMouse = [e.touches[0].clientX, e.touches[0].clientY];
        this.lastTouchDist = 0;
      }
    };

    // Attach mouse events
    this.canvas.addEventListener('mousedown', this.onMouseDown);
    window.addEventListener('mousemove', this.onMouseMove);
    window.addEventListener('mouseup', this.onMouseUp);
    this.canvas.addEventListener('wheel', this.onWheel, { passive: false });

    // Attach touch events
    this.canvas.addEventListener('touchstart', this.onTouchStart, { passive: false });
    this.canvas.addEventListener('touchmove', this.onTouchMove, { passive: false });
    this.canvas.addEventListener('touchend', this.onTouchEnd, { passive: false });
    this.canvas.addEventListener('touchcancel', this.onTouchEnd, { passive: false });
  }

  private clearLongPressTimer(): void {
    if (this.longPressTimer !== null) {
      clearTimeout(this.longPressTimer);
      this.longPressTimer = null;
    }
  }

  private rotate(dx: number, dy: number): void {
    this.camera.rotY += dx * 0.005;
    this.camera.rotX += dy * 0.005;
    this.camera.rotX = Math.max(-Math.PI / 2 + 0.01, Math.min(Math.PI / 2 - 0.01, this.camera.rotX));
  }

  private pan(dx: number, dy: number): void {
    const panSpeed = this.camera.distance * 0.002;
    const cosY = Math.cos(this.camera.rotY);
    const sinY = Math.sin(this.camera.rotY);
    this.camera.target[0] -= dx * cosY * panSpeed;
    this.camera.target[1] += dx * sinY * panSpeed;
    this.camera.target[2] += dy * panSpeed;
  }

  private zoom(factor: number): void {
    this.camera.distance *= factor;
    this.camera.distance = Math.max(0.5, Math.min(200, this.camera.distance));
  }

  reset(): void {
    this.camera.distance = DEFAULT_CAMERA.distance;
    this.camera.rotX = DEFAULT_CAMERA.rotX;
    this.camera.rotY = DEFAULT_CAMERA.rotY;
    this.camera.target = [...DEFAULT_CAMERA.target];
  }

  dispose(): void {
    this.clearLongPressTimer();
    this.canvas.removeEventListener('mousedown', this.onMouseDown);
    window.removeEventListener('mousemove', this.onMouseMove);
    window.removeEventListener('mouseup', this.onMouseUp);
    this.canvas.removeEventListener('wheel', this.onWheel);
    this.canvas.removeEventListener('touchstart', this.onTouchStart);
    this.canvas.removeEventListener('touchmove', this.onTouchMove);
    this.canvas.removeEventListener('touchend', this.onTouchEnd);
    this.canvas.removeEventListener('touchcancel', this.onTouchEnd);
  }
}
