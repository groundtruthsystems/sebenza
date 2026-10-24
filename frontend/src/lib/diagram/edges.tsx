import {
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  getSmoothStepPath,
  type Edge,
  type EdgeProps,
} from "@xyflow/react";
import type { GraphEdge, GraphKind, Point } from "./model";

export interface EdgeData extends Record<string, unknown> {
  edge: GraphEdge;
  kind: GraphKind;
}
export type DiagramEdgeType = Edge<EdgeData, "diagram">;

interface Look {
  end?: string;
  dashed?: boolean;
  thick?: boolean;
  /** Fixed text shown on the line, e.g. «include». */
  tag?: string;
}

/** How each relationship is drawn: the UML marker at the target end and the line style. */
export function lookOf(kind: GraphKind, rel: string): Look {
  switch (kind) {
    case "class":
      return (
        {
          inheritance: { end: "dg-tri" },
          realization: { end: "dg-tri", dashed: true },
          composition: { end: "dg-diamond" },
          aggregation: { end: "dg-diamond-open" },
          association: { end: "dg-open" },
          dependency: { end: "dg-open", dashed: true },
          link: {},
          linkDashed: { dashed: true },
        } as Record<string, Look>
      )[rel] ?? {};
    case "flowchart":
      return (
        {
          arrow: { end: "dg-arrow" },
          open: {},
          dotted: { end: "dg-arrow", dashed: true },
          dottedOpen: { dashed: true },
          thick: { end: "dg-arrow", thick: true },
        } as Record<string, Look>
      )[rel] ?? {};
    case "state":
      return { end: "dg-arrow" };
    case "er":
      return rel === "nonIdentifying" ? { dashed: true } : {};
    case "usecase":
      return (
        {
          association: {},
          directed: { end: "dg-open" },
          include: { end: "dg-open", dashed: true, tag: "«include»" },
          extend: { end: "dg-open", dashed: true, tag: "«extend»" },
        } as Record<string, Look>
      )[rel] ?? {};
  }
}

const ER_CARD_TEXT: Record<string, string> = { one: "1", zeroOne: "0..1", many: "0..*", oneMany: "1..*" };

/**
 * Pick the handle sides that face each other, so a line leaves a box toward
 * the box it joins rather than always from the bottom.
 */
export function sideHandles(
  a: Point & { w: number; h: number },
  b: Point & { w: number; h: number },
): { sourceHandle: string; targetHandle: string } {
  const dx = b.x + b.w / 2 - (a.x + a.w / 2);
  const dy = b.y + b.h / 2 - (a.y + a.h / 2);
  if (Math.abs(dx) > Math.abs(dy)) {
    return dx > 0 ? { sourceHandle: "s-r", targetHandle: "t-l" } : { sourceHandle: "s-l", targetHandle: "t-r" };
  }
  return dy > 0 ? { sourceHandle: "s-b", targetHandle: "t-t" } : { sourceHandle: "s-t", targetHandle: "t-b" };
}

export function DiagramEdge(props: EdgeProps<DiagramEdgeType>) {
  const { id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, data, selected } = props;
  if (!data) return null;
  const { edge, kind } = data;
  const look = lookOf(kind, edge.rel);
  const [path, labelX, labelY] =
    kind === "state"
      ? getBezierPath({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition })
      : getSmoothStepPath({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, borderRadius: 6 });
  const label = [look.tag, edge.label].filter(Boolean).join(" ");
  const card = (c: string) => (kind === "er" ? ER_CARD_TEXT[c] ?? "" : c);
  const near = (t: number) => ({
    x: sourceX + (targetX - sourceX) * t,
    y: sourceY + (targetY - sourceY) * t,
  });
  const ends = [
    { text: card(edge.sourceCard), at: near(0.12), key: "s" },
    { text: card(edge.targetCard), at: near(0.88), key: "t" },
  ].filter((e) => e.text);

  return (
    <>
      <BaseEdge
        id={id}
        path={path}
        markerEnd={look.end ? `url(#${look.end})` : undefined}
        style={{
          stroke: selected ? "var(--color-accent)" : "var(--diagram-stroke)",
          strokeWidth: look.thick ? 3 : 1.5,
          strokeDasharray: look.dashed ? "6 4" : undefined,
        }}
        interactionWidth={16}
      />
      <EdgeLabelRenderer>
        {label && (
          <div
            className="diagram-edge-label nodrag nopan"
            style={{ transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }}
          >
            {label}
          </div>
        )}
        {ends.map((e) => (
          <div
            key={e.key}
            className="diagram-edge-card nodrag nopan"
            style={{ transform: `translate(-50%, -120%) translate(${e.at.x}px, ${e.at.y}px)` }}
          >
            {e.text}
          </div>
        ))}
      </EdgeLabelRenderer>
    </>
  );
}

export const EDGE_TYPES = { diagram: DiagramEdge };

/**
 * The UML arrowheads, referenced by id from every edge. Hollow shapes are
 * filled with the canvas colour so the line does not show through them.
 */
export function MarkerDefs() {
  const common = { markerUnits: "userSpaceOnUse", orient: "auto-start-reverse" } as const;
  return (
    <svg width="0" height="0" className="absolute" aria-hidden>
      <defs>
        <marker id="dg-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="12" markerHeight="12" {...common}>
          <path d="M0,0 L10,5 L0,10 Z" fill="var(--diagram-stroke)" />
        </marker>
        <marker id="dg-open" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="12" markerHeight="12" {...common}>
          <path d="M0,0 L10,5 L0,10" fill="none" stroke="var(--diagram-stroke)" strokeWidth="1.5" />
        </marker>
        <marker id="dg-tri" viewBox="0 0 12 12" refX="11" refY="6" markerWidth="16" markerHeight="16" {...common}>
          <path d="M0,0 L12,6 L0,12 Z" fill="var(--diagram-bg)" stroke="var(--diagram-stroke)" strokeWidth="1.2" />
        </marker>
        <marker id="dg-diamond" viewBox="0 0 16 10" refX="15" refY="5" markerWidth="20" markerHeight="12" {...common}>
          <path d="M0,5 L8,0 L16,5 L8,10 Z" fill="var(--diagram-stroke)" />
        </marker>
        <marker id="dg-diamond-open" viewBox="0 0 16 10" refX="15" refY="5" markerWidth="20" markerHeight="12" {...common}>
          <path d="M0,5 L8,0 L16,5 L8,10 Z" fill="var(--diagram-bg)" stroke="var(--diagram-stroke)" strokeWidth="1.2" />
        </marker>
      </defs>
    </svg>
  );
}
