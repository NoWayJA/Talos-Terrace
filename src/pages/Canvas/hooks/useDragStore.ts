// Lifted from Modz (NoWayJA/Modz, MIT) pages/Workflows/hooks/useDragStore.ts.
// HTML5 drag and drop is off in Tauri's WKWebView (dragDropEnabled: false), so the palette
// records what is being dragged and the canvas creates the node on mouseup.
import { create } from 'zustand';
import type { PaletteEntry } from '../palette';

interface DragState {
  dragging: PaletteEntry | null;
  setDragging: (entry: PaletteEntry | null) => void;
}

export const useDragStore = create<DragState>((set) => ({
  dragging: null,
  setDragging: (entry) => {
    set({ dragging: entry });
  },
}));
