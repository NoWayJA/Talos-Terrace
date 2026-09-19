// Lifted from Modz (NoWayJA/Modz, MIT) pages/Workflows/NodePalette.tsx without the
// example-workflow section; categories collapse, entries start a mouse drag.
import { ChevronDown, ChevronRight } from 'lucide-react';
import { useState, type MouseEvent } from 'react';
import { useCanvasStore } from './hooks/useCanvasStore';
import { useDragStore } from './hooks/useDragStore';
import { PALETTE, PALETTE_CATEGORIES, type PaletteCategoryId, type PaletteEntry } from './palette';

export function NodePalette() {
  const [expanded, setExpanded] = useState<ReadonlySet<PaletteCategoryId>>(
    () => new Set<PaletteCategoryId>(['models', 'apps'])
  );
  const setDragging = useDragStore((state) => state.setDragging);
  const addPlannedNode = useCanvasStore((state) => state.addPlannedNode);

  const toggle = (id: PaletteCategoryId) => {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const handleMouseDown = (event: MouseEvent, entry: PaletteEntry) => {
    event.preventDefault();
    setDragging(entry);
  };

  return (
    <aside className="palette" aria-label="Node library">
      <div className="palette__header">
        <h2 className="palette__title">Library</h2>
        <p className="palette__hint">
          Drag onto the canvas, or press Enter on an entry, to plan a workload. Nothing is applied
          to the cluster.
        </p>
      </div>

      <div className="palette__categories">
        {PALETTE_CATEGORIES.map((category) => {
          const entries = PALETTE.filter((entry) => entry.category === category.id);
          const open = expanded.has(category.id);
          return (
            <div key={category.id} className="palette__category">
              <button
                type="button"
                className="palette__category-header"
                onClick={() => {
                  toggle(category.id);
                }}
                aria-expanded={open}
              >
                <span className="palette__category-label">{category.label}</span>
                <span className="palette__category-chevron">
                  {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                </span>
              </button>

              {open ? (
                <div className="palette__nodes">
                  {entries.map((entry) => (
                    <div
                      key={entry.id}
                      className="palette__node"
                      role="button"
                      tabIndex={0}
                      aria-describedby={`palette-${entry.id}-description`}
                      onMouseDown={(event) => {
                        handleMouseDown(event, entry);
                      }}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter' || event.key === ' ') {
                          event.preventDefault();
                          addPlannedNode(entry, null);
                        }
                      }}
                    >
                      <div className="palette__node-content">
                        <span className="palette__node-label">{entry.label}</span>
                        <span
                          id={`palette-${entry.id}-description`}
                          className="palette__node-description"
                        >
                          {entry.description}
                        </span>
                      </div>
                      {entry.wantsGpu ? <span className="badge badge--success">GPU</span> : null}
                    </div>
                  ))}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
    </aside>
  );
}
