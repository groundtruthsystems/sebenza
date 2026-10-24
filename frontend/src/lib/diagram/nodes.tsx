import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { Handle, NodeResizer, Position, type Node, type NodeProps } from "@xyflow/react";
import type { GraphKind, GraphNode } from "./model";

/** What a node component may ask of the canvas that owns it. */
export interface CanvasActions {
  rename(id: string, label: string): void;
  resize(id: string, pos: { x: number; y: number }, w: number, h: number): void;
}
export const CanvasActionsContext = createContext<CanvasActions | null>(null);

export interface NodeData extends Record<string, unknown> {
  node: GraphNode;
  kind: GraphKind;
}
export type ShapeNodeType = Node<NodeData, "shape">;
export type GroupNodeType = Node<NodeData, "group">;

const SIDES = [
  ["t", Position.Top],
  ["r", Position.Right],
  ["b", Position.Bottom],
  ["l", Position.Left],
] as const;

/**
 * A source and a target handle on every side. Edges in Mermaid have no
 * attachment point, so the canvas picks the sides facing each other
 * (`sideHandles` in edges.tsx); these are what it picks from.
 */
function Handles() {
  return (
    <>
      {SIDES.map(([id, pos]) => (
        <Handle key={`s-${id}`} id={`s-${id}`} type="source" position={pos} className="diagram-handle" />
      ))}
      {SIDES.map(([id, pos]) => (
        <Handle key={`t-${id}`} id={`t-${id}`} type="target" position={pos} className="diagram-handle" />
      ))}
    </>
  );
}

/** Text that turns into an input on double-click; Enter or blur commits. */
function InlineLabel({ id, value, className = "" }: { id: string; value: string; className?: string }) {
  const actions = useContext(CanvasActionsContext);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);

  if (!editing) {
    return (
      <span
        className={className}
        onDoubleClick={(e) => {
          e.stopPropagation();
          setDraft(value);
          setEditing(true);
        }}
        title="Double-click to rename"
      >
        {value || " "}
      </span>
    );
  }
  const commit = () => {
    setEditing(false);
    if (draft !== value) actions?.rename(id, draft);
  };
  return (
    <input
      ref={input}
      aria-label="Node label"
      className="nodrag bg-surface border border-accent rounded px-1 text-xs text-primary w-full text-center"
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setEditing(false);
      }}
    />
  );
}

const frame = (selected: boolean) =>
  `diagram-node ${selected ? "diagram-node-selected" : ""}`;

/** An SVG outline behind a label, for shapes CSS borders cannot draw. */
function Outline({ shape, children, selected }: { shape: string; children: ReactNode; selected: boolean }) {
  const path: Record<string, ReactNode> = {
    decision: <polygon points="50,1 99,50 50,99 1,50" />,
    io: <polygon points="12,1 99,1 88,99 1,99" />,
    database: (
      <>
        <path d="M1,12 C1,-3 99,-3 99,12 L99,88 C99,103 1,103 1,88 Z" />
        <path d="M1,12 C1,27 99,27 99,12" fill="none" />
      </>
    ),
  };
  return (
    <div className={`relative flex items-center justify-center px-5 py-3 min-w-[120px] min-h-[48px] ${shape === "decision" ? "min-h-[80px] px-8" : ""}`}>
      <svg
        className={`absolute inset-0 w-full h-full overflow-visible diagram-outline ${selected ? "diagram-outline-selected" : ""}`}
        viewBox="0 0 100 100"
        preserveAspectRatio="none"
        aria-hidden
      >
        {path[shape]}
      </svg>
      <div className="relative text-xs text-center">{children}</div>
    </div>
  );
}

function ClassBox({ node, selected }: { node: GraphNode; selected: boolean }) {
  const stereo: Record<string, string> = {
    interface: "«interface»",
    abstract: "«abstract»",
    enum: "«enumeration»",
  };
  return (
    <div className={`${frame(selected)} min-w-[140px] text-xs`}>
      <div className="px-3 py-1.5 text-center border-b border-[var(--diagram-line)]">
        {stereo[node.shape] && <div className="text-[10px] text-muted">{stereo[node.shape]}</div>}
        <InlineLabel id={node.id} value={node.label} className={`font-semibold ${node.shape === "abstract" ? "italic" : ""}`} />
      </div>
      <div className="px-2 py-1 min-h-[14px] font-mono text-[11px]">
        {node.attributes.map((a, i) => (
          <div key={i}>{a}</div>
        ))}
      </div>
      {node.shape !== "enum" && (
        <div className="px-2 py-1 min-h-[14px] border-t border-[var(--diagram-line)] font-mono text-[11px]">
          {node.methods.map((m, i) => (
            <div key={i}>{m}</div>
          ))}
        </div>
      )}
    </div>
  );
}

function EntityBox({ node, selected }: { node: GraphNode; selected: boolean }) {
  return (
    <div className={`${frame(selected)} min-w-[140px] text-xs`}>
      <div className="px-3 py-1.5 text-center font-semibold border-b border-[var(--diagram-line)]">
        <InlineLabel id={node.id} value={node.label} />
      </div>
      <table className="w-full font-mono text-[11px]">
        <tbody>
          {node.attributes.map((a, i) => {
            const [type, name, ...rest] = a.split(/\s+/);
            return (
              <tr key={i} className="border-t border-[var(--diagram-line)] first:border-t-0">
                <td className="px-2 py-0.5 text-muted">{type}</td>
                <td className="px-2 py-0.5">{name}</td>
                <td className="px-2 py-0.5 text-accent">{rest.join(" ")}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function Actor({ node, selected }: { node: GraphNode; selected: boolean }) {
  return (
    <div className="flex flex-col items-center gap-1 px-2 text-xs">
      <svg width="30" height="48" viewBox="0 0 30 48" className={`diagram-outline ${selected ? "diagram-outline-selected" : ""}`} aria-hidden>
        <circle cx="15" cy="8" r="7" />
        <path d="M15 15 V32 M2 21 H28 M15 32 L4 47 M15 32 L26 47" fill="none" />
      </svg>
      <InlineLabel id={node.id} value={node.label} />
    </div>
  );
}

export function ShapeNode({ data, selected }: NodeProps<ShapeNodeType>) {
  const { node, kind } = data;
  const sel = !!selected;
  let body: ReactNode;
  if (kind === "class") {
    body = <ClassBox node={node} selected={sel} />;
  } else if (kind === "er") {
    body = <EntityBox node={node} selected={sel} />;
  } else if (node.shape === "actor") {
    body = <Actor node={node} selected={sel} />;
  } else if (node.shape === "start") {
    body = <div className={`w-7 h-7 rounded-full bg-primary ${sel ? "ring-2 ring-accent" : ""}`} title="Initial state" />;
  } else if (node.shape === "end") {
    body = (
      <div className={`w-7 h-7 rounded-full border-2 border-primary flex items-center justify-center ${sel ? "ring-2 ring-accent" : ""}`} title="Final state">
        <div className="w-4 h-4 rounded-full bg-primary" />
      </div>
    );
  } else if (node.shape === "choice") {
    body = (
      <div className="w-7 h-7 flex items-center justify-center" title="Choice">
        <div className={`w-5 h-5 rotate-45 border-2 bg-sidebar ${sel ? "border-accent" : "border-[var(--diagram-stroke)]"}`} />
      </div>
    );
  } else if (["decision", "io", "database"].includes(node.shape)) {
    body = (
      <Outline shape={node.shape} selected={sel}>
        <InlineLabel id={node.id} value={node.label} />
      </Outline>
    );
  } else {
    const round: Record<string, string> = {
      rounded: "rounded-lg",
      terminal: "rounded-full",
      usecase: "rounded-[50%] px-6 py-4",
      circle: "rounded-full aspect-square",
      state: "rounded-xl",
    };
    body = (
      <div className={`${frame(sel)} ${round[node.shape] ?? "rounded-sm"} px-4 py-2.5 min-w-[110px] text-xs text-center flex items-center justify-center`}>
        <InlineLabel id={node.id} value={node.label} />
      </div>
    );
  }
  return (
    <>
      <Handles />
      {body}
    </>
  );
}

/** Subgraphs, composite states and system boundaries: resizable containers. */
export function GroupNode({ id, data, selected }: NodeProps<GroupNodeType>) {
  const actions = useContext(CanvasActionsContext);
  return (
    <>
      <NodeResizer
        isVisible={!!selected}
        minWidth={140}
        minHeight={90}
        lineClassName="diagram-resize-line"
        handleClassName="diagram-resize-handle"
        onResizeEnd={(_e, p) => actions?.resize(id, { x: p.x, y: p.y }, p.width, p.height)}
      />
      <Handles />
      <div
        className={`w-full h-full diagram-group ${data.kind === "state" ? "rounded-xl" : "rounded-md"} ${selected ? "diagram-node-selected" : ""}`}
      >
        <div className="px-3 py-1.5 text-xs font-semibold text-muted">
          <InlineLabel id={data.node.id} value={data.node.label} />
        </div>
      </div>
    </>
  );
}

export const NODE_TYPES = { shape: ShapeNode, group: GroupNode };
