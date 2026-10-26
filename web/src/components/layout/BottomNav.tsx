import type { ViewMode } from '@state/app-store.ts';

export type MobileTab = '3d' | '2d' | 'code' | 'chat' | 'properties';

interface BottomNavProps {
  activeTab: MobileTab;
  onTabChange: (tab: MobileTab) => void;
  viewMode: ViewMode;
}

interface NavItem {
  tab: MobileTab;
  icon: string;
  label: string;
}

const navItems: NavItem[] = [
  { tab: '3d', icon: '\u25B3', label: '3D' },       // triangle
  { tab: '2d', icon: '\u25A1', label: '2D' },       // square
  { tab: 'code', icon: '\u2702', label: 'Code' },   // scissors → code symbol
  { tab: 'chat', icon: '\u2709', label: 'Chat' },   // envelope
  { tab: 'properties', icon: '\u2699', label: 'Info' }, // gear
];

/**
 * Bottom navigation bar for mobile with icon buttons.
 * Switches between 3D/2D/Code/Chat/Properties views.
 */
export function BottomNav({ activeTab, onTabChange }: BottomNavProps) {
  return (
    <div style={{
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-around',
      height: 56,
      minHeight: 56,
      background: 'var(--surface)',
      borderTop: '1px solid var(--border)',
      paddingBottom: 'env(safe-area-inset-bottom, 0px)',
      touchAction: 'none',
    }}>
      {navItems.map(({ tab, icon, label }) => {
        const isActive = activeTab === tab;
        return (
          <button
            key={tab}
            onClick={() => onTabChange(tab)}
            style={{
              flex: 1,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 2,
              background: 'transparent',
              border: 'none',
              cursor: 'pointer',
              padding: '6px 0',
              color: isActive ? 'var(--accent)' : 'var(--text-dim)',
              transition: 'color 0.15s ease',
              fontFamily: 'inherit',
              minHeight: 44,
            }}
          >
            <span style={{ fontSize: 18, lineHeight: 1 }}>{icon}</span>
            <span style={{
              fontSize: 10,
              fontWeight: isActive ? 600 : 400,
              letterSpacing: 0.3,
            }}>
              {label}
            </span>
          </button>
        );
      })}
    </div>
  );
}
