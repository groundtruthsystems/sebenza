import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  applyNodeChanges,
  Background,
  ConnectionMode,
  Controls,
  ReactFlow,
  useReactFlow,
  type Connection,
  type EdgeChange,
  type FinalConnectionState,
  type Node,
  type NodeChange,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import Btn from "../Btn";
import GraphInspector from "./Inspector";
import { autoLayout, estimateSize, type Size } from "./layout";
import { isGroup, KIND_SPECS, parentsFirst, type GraphDoc, type GraphNode } from "./model";
import { CanvasActionsContext, NODE_TYPES, type CanvasActions, type NodeData } from "./nodes";
import { EDGE_TYPES, MarkerDefs, sideHandles, type DiagramEdgeType } from "./edges";
import {
  absolutePositions,
  addEdge,
  addNode,
  deleteItems,
  moveNodes,
  resizeNode,
  reverseEdge,
  setParent,
  updateEdge,
  updateNode,
} from "./ops";

export interface ChangeOpts {
  /** Successive changes with the same key fold into one undo step. */
  coalesce?: string;
  /** Not a user edit (e.g. the first layout): replaces the current step. */
  silent?: boolean;
}

interface Props {
  doc: GraphDoc;
  onChange(doc: GraphDoc, opts?: ChangeOpts): void;
}

type RfNode = Node<NodeData>;

/** Doc nodes as React Flow nodes, keeping what React Flow measured last time. */
function toRfNodes(doc: GraphDoc, prev: RfNode[], select: Set<string> | null): RfNode[] {
  const old = new Map(prev.map((n) => [n.id, n]));
  return parentsFirst(doc.nodes).map((n) => {
    const was = old.get(n.id);
    const group = isGroup(doc, n);
    return {
      id: n.id,
      type: group ? "group" : "shape",
      position: n.pos ?? { x: 0, y: 0 },
      parentId: n.parent,
      data: { node: n, kind: doc.kind },
      selected: select ? select.has(n.id) : was?.selected ?? false,
      measured: was?.measured,
      ...(group ? { style: { width: n.size?.w ?? 280, height: n.size?.h ?? 180 }, zIndex: -1 } : {}),
    } as RfNode;
  });
}

export default function GraphCanvas({ doc, onChange }: Props) {
  const flow = useReactFlow();
  const wrapper = useRef<HTMLDivElement>(null);
  const spec = KIND_SPECS[doc.kind];
  const [rfNodes, setRfNodes] = useState<RfNode[]>(() => toRfNodes(doc, [], null));
  const [selectedEdges, setSelectedEdges] = useState<Set<string>>(new Set());
  /** Set when an edit re-keys or creates a node that should end up selected. */
  const pendingSelect = useRef<Set<string> | null>(null);

  useEffect(() => {
    // Read before clearing: the updater runs later, during the next render.
    const select = pendingSelect.current;
    pendingSelect.current = null;
    setRfNodes((prev) => toRfNodes(doc, prev, select));
  }, [doc]);

  const measured = useMemo(() => {
    const m = new Map<string, Size>();
    for (const n of rfNodes) {
      if (n.measured?.width && n.measured?.height) m.set(n.id, { width: n.measured.width, height: n.measured.height });
    }
    return m;
  }, [rfNodes]);

  const rfEdges: DiagramEdgeType[] = useMemo(() => {
    const abs = absolutePositions(doc);
    const byId = new Map(doc.nodes.map((n) => [n.id, n]));
    const box = (id: string) => {
      const n = byId.get(id) as GraphNode;
      const s = measured.get(id) ?? estimateSize(doc, n);
      return { ...(abs.get(id) ?? { x: 0, y: 0 }), w: s.width, h: s.height };
    };
    return doc.edges
      .filter((e) => byId.has(e.source) && byId.has(e.target))
      .map((e) => ({
        id: e.id,
        type: "diagram",
        source: e.source,
        target: e.target,
        ...sideHandles(box(e.source), box(e.target)),
        selected: selectedEdges.has(e.id),
        data: { edge: e, kind: doc.kind },
      }));
  }, [doc, measured, selectedEdges]);

  const selectedNode = useMemo(() => {
    const sel = rfNodes.filter((n) => n.selected);
    return sel.length === 1 ? doc.nodes.find((n) => n.id === sel[0].id) ?? null : null;
  }, [rfNodes, doc]);
  const selectedEdge = useMemo(
    () => (selectedEdges.size === 1 && !selectedNode ? doc.edges.find((e) => selectedEdges.has(e.id)) ?? null : null),
    [selectedEdges, selectedNode, doc],
  );

  // Removals are left to `onDelete`, which reports nodes and edges together;
  // handling them here as well would apply two deletes to the same old doc.
  const onNodesChange = useCallback((changes: NodeChange<RfNode>[]) => {
    setRfNodes((ns) => applyNodeChanges(changes.filter((c) => c.type !== "remove"), ns));
  }, []);

  const onEdgesChange = useCallback((changes: EdgeChange<DiagramEdgeType>[]) => {
    setSelectedEdges((prev) => {
      const next = new Set(prev);
      for (const c of changes) {
        if (c.type !== "select") continue;
        if (c.selected) next.add(c.id);
        else next.delete(c.id);
      }
      return next;
    });
  }, []);

  const onDelete = useCallback(
    ({ nodes, edges }: { nodes: RfNode[]; edges: DiagramEdgeType[] }) => {
      onChange(deleteItems(doc, nodes.map((n) => n.id), edges.map((e) => e.id)));
      setSelectedEdges(new Set());
    },
    [doc, onChange],
  );

  const onConnect = useCallback(
    (c: Connection) => {
      if (!c.source || !c.target) return;
      const { doc: next, id } = addEdge(doc, c.source, c.target);
      onChange(next);
      setSelectedEdges(new Set([id]));
      pendingSelect.current = new Set();
    },
    [doc, onChange],
  );

  /**
   * React Flow only connects when the drag ends on a handle. Dropping on the
   * body of a box is the more natural gesture, so that connects too.
   */
  const onConnectEnd = useCallback(
    (e: MouseEvent | TouchEvent, state: FinalConnectionState) => {
      if (state.isValid || !state.fromNode) return;
      const point = "changedTouches" in e ? e.changedTouches[0] : e;
      const el = document.elementFromPoint(point.clientX, point.clientY);
      const target = el?.closest<HTMLElement>(".react-flow__node")?.dataset.id;
      if (target && target !== state.fromNode.id) {
        onConnect({ source: state.fromNode.id, target, sourceHandle: null, targetHandle: null });
      }
    },
    [onConnect],
  );

  const onNodeDragStop = useCallback(
    (_e: unknown, node: RfNode, dragged: RfNode[]) => {
      let next = moveNodes(doc, new Map(dragged.map((n) => [n.id, n.position])));
      // Dropping onto a group moves the node into it; dropping outside
      // every group moves it out to the top level.
      if (spec.groupShape) {
        const mine = new Set([node.id]);
        const target = flow
          .getIntersectingNodes(node)
          .filter((n) => n.type === "group" && !mine.has(n.id))
          .filter((n) => !isDescendant(next, n.id, node.id))
          .pop();
        if ((target?.id ?? undefined) !== node.parentId) {
          const moved = setParent(next, node.id, target?.id);
          next = moved.doc;
          pendingSelect.current = new Set([moved.id]);
        }
      }
      onChange(next);
    },
    [doc, flow, onChange, spec.groupShape],
  );

  const actions: CanvasActions = useMemo(
    () => ({
      rename(id, label) {
        const r = updateNode(doc, id, { label });
        pendingSelect.current = new Set([r.id]);
        onChange(r.doc);
      },
      resize(id, pos, w, h) {
        onChange(resizeNode(doc, id, pos, w, h));
      },
    }),
    [doc, onChange],
  );

  const add = (shape: string) => {
    // New nodes go into a selected group, or the middle of the view.
    const group = selectedNode && isGroup(doc, selectedNode) && shape !== spec.groupShape ? selectedNode : null;
    let pos: { x: number; y: number };
    if (group) {
      const kids = doc.nodes.filter((n) => n.parent === group.id).length;
      pos = { x: 24 + (kids % 3) * 40, y: 44 + (kids % 3) * 30 };
    } else {
      const r = wrapper.current?.getBoundingClientRect();
      const centre = r ? flow.screenToFlowPosition({ x: r.left + r.width / 2, y: r.top + r.height / 2 }) : { x: 0, y: 0 };
      const jitter = (doc.nodes.length % 5) * 18;
      pos = { x: centre.x - 60 + jitter, y: centre.y - 24 + jitter };
    }
    const { doc: next, id } = addNode(doc, shape, pos, group?.id);
    pendingSelect.current = new Set([id]);
    setSelectedEdges(new Set());
    onChange(next);
  };

  const deleteSelected = () => {
    const nodes = rfNodes.filter((n) => n.selected).map((n) => n.id);
    onChange(deleteItems(doc, nodes, selectedEdges));
    setSelectedEdges(new Set());
  };

  const relayout = () => {
    onChange(autoLayout(doc, measured));
    requestAnimationFrame(() => void flow.fitView({ padding: 0.2, duration: 200 }));
  };

  const palette = spec.shapes;
  const hasSelection = rfNodes.some((n) => n.selected) || selectedEdges.size > 0;

  return (
    <CanvasActionsContext.Provider value={actions}>
      <div className="flex-1 min-h-0 flex">
        <div className="flex-1 min-w-0 flex flex-col">
          <div className="flex flex-wrap items-center gap-1.5 px-3 py-2 border-b border-edge bg-sidebar" role="toolbar" aria-label="Diagram tools">
            {palette.map((s) => (
              <Btn key={s.value} small onClick={() => add(s.value)}>
                + {s.label}
              </Btn>
            ))}
            <span className="mx-1 h-4 w-px bg-edge" />
            <Btn small onClick={relayout} title="Arrange automatically">
              Auto-layout
            </Btn>
            <Btn small variant="danger-outline" disabled={!hasSelection} onClick={deleteSelected}>
              Delete
            </Btn>
          </div>
          <div ref={wrapper} className="flex-1 min-h-0 relative diagram-canvas">
            <MarkerDefs />
            <ReactFlow
              nodes={rfNodes}
              edges={rfEdges}
              nodeTypes={NODE_TYPES}
              edgeTypes={EDGE_TYPES}
              onNodesChange={onNodesChange}
              onEdgesChange={onEdgesChange}
              onConnect={onConnect}
              onConnectEnd={onConnectEnd}
              onDelete={onDelete}
              onNodeDragStop={onNodeDragStop}
              connectionMode={ConnectionMode.Loose}
              deleteKeyCode={["Delete", "Backspace"]}
              colorMode="dark"
              fitView
              fitViewOptions={{ padding: 0.2 }}
              minZoom={0.2}
              proOptions={{ hideAttribution: true }}
            >
              <Background gap={16} />
              <Controls showInteractive={false} />
            </ReactFlow>
          </div>
        </div>
        <aside className="w-64 shrink-0 border-l border-edge bg-sidebar p-3 overflow-y-auto" aria-label="Inspector">
          <GraphInspector
            doc={doc}
            node={selectedNode}
            edge={selectedEdge}
            onNode={(id, patch, coalesce) => {
              const r = updateNode(doc, id, patch);
              if (r.id !== id) pendingSelect.current = new Set([r.id]);
              onChange(r.doc, { coalesce });
            }}
            onParent={(id, parent) => {
              const r = setParent(doc, id, parent);
              pendingSelect.current = new Set([r.id]);
              onChange(r.doc);
            }}
            onEdge={(id, patch, coalesce) => onChange(updateEdge(doc, id, patch), { coalesce })}
            onReverse={(id) => onChange(reverseEdge(doc, id))}
            onDirection={(direction) => onChange({ ...doc, direction })}
            onDelete={deleteSelected}
          />
        </aside>
      </div>
    </CanvasActionsContext.Provider>
  );
}

function isDescendant(doc: GraphDoc, maybeChild: string, ancestor: string): boolean {
  const byId = new Map(doc.nodes.map((n) => [n.id, n]));
  let cur = byId.get(maybeChild)?.parent;
  for (let i = 0; cur && i < doc.nodes.length; i++) {
    if (cur === ancestor) return true;
    cur = byId.get(cur)?.parent;
  }
  return false;
}
