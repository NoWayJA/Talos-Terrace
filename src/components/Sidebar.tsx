// Structure lifted from Modz (NoWayJA/Modz, MIT) components/Sidebar.tsx, reduced to two pages.
import { ChevronLeft, ChevronRight, Layers, Settings } from 'lucide-react';
import type { ReactNode } from 'react';
import type { Page } from '@/pages';

interface NavItem {
  id: Page;
  label: string;
  icon: ReactNode;
}

const NAV_ITEMS: readonly NavItem[] = [
  { id: 'canvas', label: 'Canvas', icon: <Layers size={18} /> },
  { id: 'settings', label: 'Settings', icon: <Settings size={18} /> },
];

interface SidebarProps {
  current: Page;
  onNavigate: (page: Page) => void;
  collapsed: boolean;
  onToggle: () => void;
  version: string;
}

export function Sidebar({ current, onNavigate, collapsed, onToggle, version }: SidebarProps) {
  return (
    <nav className={collapsed ? 'sidebar sidebar--collapsed' : 'sidebar'} aria-label="Primary">
      <div className="sidebar__brand">
        <span className="sidebar__logo" aria-hidden="true">
          T
        </span>
        <span className="sidebar__brand-name">Talos Terrace</span>
        <button
          type="button"
          className="sidebar__toggle"
          onClick={onToggle}
          aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          {collapsed ? <ChevronRight size={16} /> : <ChevronLeft size={16} />}
        </button>
      </div>

      <div className="sidebar__nav">
        {NAV_ITEMS.map((item) => {
          const active = item.id === current;
          return (
            <button
              key={item.id}
              type="button"
              className={
                active ? 'sidebar__nav-item sidebar__nav-item--active' : 'sidebar__nav-item'
              }
              onClick={() => {
                onNavigate(item.id);
              }}
              aria-current={active ? 'page' : undefined}
              title={item.label}
            >
              <span className="sidebar__nav-icon">{item.icon}</span>
              <span className="sidebar__nav-label">{item.label}</span>
            </button>
          );
        })}
      </div>

      <div className="sidebar__footer">
        {version ? <span className="sidebar__version">v{version}</span> : null}
      </div>
    </nav>
  );
}
