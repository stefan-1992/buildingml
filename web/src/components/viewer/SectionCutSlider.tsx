import { useViewerStore } from '@state/viewer-store.ts';

const panelStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  background: 'rgba(0,0,0,0.55)',
  padding: '8px 6px',
  borderRadius: 6,
  fontFamily: 'monospace',
  fontSize: 11,
  color: 'rgba(255,255,255,0.75)',
};

const checkboxLabelStyle: React.CSSProperties = {
  cursor: 'pointer',
  userSelect: 'none',
  display: 'flex',
  alignItems: 'center',
  gap: 4,
};

export function SectionCutSlider() {
  const sectionCut = useViewerStore((s) => s.sectionCut);
  const setSectionCut = useViewerStore((s) => s.setSectionCut);
  const lateralCut = useViewerStore((s) => s.lateralCut);
  const setLateralCut = useViewerStore((s) => s.setLateralCut);

  return (
    <>
      {/* Elevation cut (Z) — vertical slider, right side */}
      <div style={{
        ...panelStyle,
        position: 'absolute',
        right: 12,
        top: '50%',
        transform: 'translateY(-50%)',
        flexDirection: 'column',
      }}>
        <label style={checkboxLabelStyle}>
          <input
            type="checkbox"
            checked={sectionCut.enabled}
            onChange={(e) => setSectionCut({ enabled: e.target.checked })}
            style={{ cursor: 'pointer' }}
          />
          Plan
        </label>
        <input
          type="range"
          min={sectionCut.min}
          max={sectionCut.max}
          step={sectionCut.step}
          value={sectionCut.value}
          onChange={(e) => setSectionCut({ value: parseFloat(e.target.value) })}
          style={{
            writingMode: 'vertical-lr',
            direction: 'rtl',
            height: 150,
            cursor: 'pointer',
          }}
          disabled={!sectionCut.enabled}
        />
        <span style={{ minWidth: 48, textAlign: 'center' }}>
          {sectionCut.value.toFixed(1)} m
        </span>
      </div>

      {/* Lateral cut (Y) — horizontal slider, bottom-right */}
      <div style={{
        ...panelStyle,
        position: 'absolute',
        bottom: 12,
        right: 12,
      }}>
        <label style={checkboxLabelStyle}>
          <input
            type="checkbox"
            checked={lateralCut.enabled}
            onChange={(e) => setLateralCut({ enabled: e.target.checked })}
            style={{ cursor: 'pointer' }}
          />
          Section
        </label>
        <input
          type="range"
          min={lateralCut.min}
          max={lateralCut.max}
          step={lateralCut.step}
          value={lateralCut.value}
          onChange={(e) => setLateralCut({ value: parseFloat(e.target.value) })}
          style={{ width: 120, cursor: 'pointer' }}
          disabled={!lateralCut.enabled}
        />
        <span style={{ minWidth: 48, textAlign: 'right' }}>
          {lateralCut.value.toFixed(1)} m
        </span>
      </div>
    </>
  );
}
