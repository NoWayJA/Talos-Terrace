// Lifted from Modz (NoWayJA/Modz, MIT) pages/Workflows/WorkflowCanvas.tsx.
// Kept: the provider wrapper, module-level nodeTypes, store-backed change handlers and the
// mouse-drop handler. Dropped: execution status, auto-pan, animated edges.
import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  type EdgeMouseHandler,
  type NodeMouseHandler,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { useCallback, useEffect, type MouseEvent } from 'react';
import { useCanvasStore } from './hooks/useCanvasStore';
import { useDragStore } from './hooks/useDragStore';
import { LAYER_COLORS } from './layers';
import { nodeTypes } from './nodes';
import type { CanvasNode } from './nodes/types';

function ClusterCanvasInner() {
  const nodes = useCanvasStore((state) => state.nodes);
  const edges = useCanvasStore((state) => state.edges);
  const onNodesChange = useCanvasStore((state) => state.onNodesChange);
  const onEdgesChange = useCanvasStore((state) => state.onEdgesChange);
  const selectNode = useCanvasStore((state) => state.selectNode);
  const selectEdge = useCanvasStore((state) => state.selectEdge);
  const addPlannedNode = useCanvasStore((state) => state.addPlannedNode);
  const { screenToFlowPosition } = useReactFlow<CanvasNode>();

  // Mouse-based drop: the palette sets the dragging entry on mousedown, the canvas creates
  // the node on mouseup. HTML5 drag and drop is blocked by Tauri's WKWebView.
  const handleMouseUp = useCallback(
    (event: MouseEvent<HTMLDivElement>) => {
      const dragging = useDragStore.getState().dragging;
      if (!dragging) return;

      const position = screenToFlowPosition({ x: event.clientX, y: event.clientY });
      if (Number.isNaN(position.x) || Number.isNaN(position.y)) return;

      addPlannedNode(dragging, position);
      useDragStore.getState().setDragging(null);
    },
    [screenToFlowPosition, addPlannedNode]
  );

  const handleMouseLeave = useCallback(() => {
    useDragStore.getState().setDragging(null);
  }, []);

  // A mouseup anywhere else ends the drag too.
  useEffect(() => {
    const clear = () => {
      useDragStore.getState().setDragging(null);
    };
    window.addEventListener('mouseup', clear);
    return () => {
      window.removeEventListener('mouseup', clear);
    };
  }, []);

  const handleNodeClick: NodeMouseHandler<CanvasNode> = useCallback(
    (_event, node) => {
      if (node.type === 'fact') selectNode(node.id);
    },
    [selectNode]
  );

  const handleEdgeClick: EdgeMouseHandler = useCallback(
    (_event, edge) => {
      selectEdge(edge.id);
    },
    [selectEdge]
  );

  const handlePaneClick = useCallback(() => {
    selectNode(null);
  }, [selectNode]);

  return (
    <div className="terrace-canvas" onMouseUp={handleMouseUp} onMouseLeave={handleMouseLeave}>
      <ReactFlow<CanvasNode>
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onNodeClick={handleNodeClick}
        onEdgeClick={handleEdgeClick}
        onPaneClick={handlePaneClick}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.08 }}
        minZoom={0.2}
        maxZoom={2}
        snapToGrid
        snapGrid={[12, 12]}
        nodesConnectable={false}
        deleteKeyCode={['Backspace', 'Delete']}
      >
        <Background gap={24} size={1} />
        <Controls showInteractive={false} />
        <MiniMap<CanvasNode>
          nodeColor={(node) => LAYER_COLORS[node.data.layer]}
          nodeStrokeWidth={2}
          pannable
          zoomable
        />
      </ReactFlow>
    </div>
  );
}

export function ClusterCanvas() {
  return (
    <ReactFlowProvider>
      <ClusterCanvasInner />
    </ReactFlowProvider>
  );
}
