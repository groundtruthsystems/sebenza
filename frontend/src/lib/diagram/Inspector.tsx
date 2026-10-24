import { useEffect, useState, type ReactNode } from "react";
import Btn from "../Btn";
import {
  descendantsOf,
  ER_CARDS,
  isGroup,
  KIND_SPECS,
  UNLABELLED_SHAPES,
  type Direction,
  type GraphDoc,
  type GraphEdge,
  type GraphNode,
} from "./model";

/** Shared form controls, styled once for both inspectors. */
export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1 text-[11px] text-muted">
      {label}
      {children}
    </label>
  );
}
export const inputClass =
  "bg-surface border border-edge rounded px-2 py-1 text-xs text-primary focus:outline-none focus:border-accent";

export const DIRECTIONS: { value: Direction; label: string }[] = [
  { value: "TB", label: "Top to bottom" },
  { value: "LR", label: "Left to right" },
  { value: "BT", label: "Bottom to top" },
  { value: "RL", label: "Right to left" },
];

/**
 * An input that reports only when editing ends (blur or Enter). For class and
 * entity names, where every change re-keys the node, committing per keystroke
 * would rewrite the name under the cursor.
 */
function CommitInput({ value, onCommit, label }: { value: string; onCommit: (v: string) => void; label: string }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft.trim() && draft !== value) onCommit(draft);
    else setDraft(value);
  };
  return (
    <input
      className={inputClass}
      aria-label={label}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setDraft(value);
      }}
    />
  );
}

/** One item per line, blank lines dropped. */
function Lines({ label, value, onChange, placeholder }: { label: string; value: string[]; onChange: (v: string[]) => void; placeholder: string }) {
  return (
    <Field label={label}>
      <textarea
        className={`${inputClass} font-mono min-h-[72px] resize-y`}
        value={value.join("\n")}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value.split("\n"))}
        onBlur={(e) => onChange(e.target.value.split("\n").map((l) => l.trim()).filter(Boolean))}
        spellCheck={false}
      />
    </Field>
  );
}

export interface GraphInspectorProps {
  doc: GraphDoc;
  node: GraphNode | null;
  edge: GraphEdge | null;
  onNode(id: string, patch: Partial<Omit<GraphNode, "id">>, coalesce?: string): void;
  onParent(id: string, parent: string | undefined): void;
  onEdge(id: string, patch: Partial<Omit<GraphEdge, "id">>, coalesce?: string): void;
  onReverse(id: string): void;
  onDirection(d: Direction): void;
  onDelete(): void;
}

export default function GraphInspector(p: GraphInspectorProps) {
  const spec = KIND_SPECS[p.doc.kind];
  const nameOf = (id: string) => p.doc.nodes.find((n) => n.id === id)?.label || id;

  if (p.node) {
    const n = p.node;
    const groups = p.doc.nodes.filter((g) => isGroup(p.doc, g) && !descendantsOf(p.doc, n.id).has(g.id));
    const group = isGroup(p.doc, n);
    return (
      <div className="flex flex-col gap-3">
        <h3 className="text-xs font-semibold">{spec.shapes.find((s) => s.value === n.shape)?.label ?? n.shape}</h3>
        {!UNLABELLED_SHAPES.has(n.shape) && (
          <Field label={spec.namedById ? "Name (letters, digits, _)" : "Label"}>
            {spec.namedById ? (
              <CommitInput label="Name" value={n.label} onCommit={(label) => p.onNode(n.id, { label })} />
            ) : (
              <input
                className={inputClass}
                aria-label="Name"
                value={n.label}
                onChange={(e) => p.onNode(n.id, { label: e.target.value }, `label:${n.id}`)}
              />
            )}
          </Field>
        )}
        {!group && !UNLABELLED_SHAPES.has(n.shape) && spec.shapes.length > 1 && (
          <Field label="Type">
            <select
              className={inputClass}
              value={n.shape}
              onChange={(e) => p.onNode(n.id, { shape: e.target.value })}
            >
              {spec.shapes
                .filter((s) => s.value !== spec.groupShape && !UNLABELLED_SHAPES.has(s.value))
                .map((s) => (
                  <option key={s.value} value={s.value}>
                    {s.label}
                  </option>
                ))}
            </select>
          </Field>
        )}
        {spec.hasAttributes && (
          <Lines
            label={p.doc.kind === "er" ? "Attributes (type name PK|FK|UK)" : n.shape === "enum" ? "Values" : "Attributes"}
            placeholder={p.doc.kind === "er" ? "string email UK" : "+String name"}
            value={n.attributes}
            onChange={(attributes) => p.onNode(n.id, { attributes }, `attrs:${n.id}`)}
          />
        )}
        {spec.hasMethods && n.shape !== "enum" && (
          <Lines
            label="Methods"
            placeholder="+save() bool"
            value={n.methods}
            onChange={(methods) => p.onNode(n.id, { methods }, `methods:${n.id}`)}
          />
        )}
        {spec.groupShape && (
          <Field label="Inside">
            <select
              className={inputClass}
              value={n.parent ?? ""}
              onChange={(e) => p.onParent(n.id, e.target.value || undefined)}
            >
              <option value="">(top level)</option>
              {groups.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.label || g.id}
                </option>
              ))}
            </select>
          </Field>
        )}
        <Btn small variant="danger-outline" onClick={p.onDelete}>
          Delete
        </Btn>
      </div>
    );
  }

  if (p.edge) {
    const e = p.edge;
    return (
      <div className="flex flex-col gap-3">
        <h3 className="text-xs font-semibold">
          {nameOf(e.source)} → {nameOf(e.target)}
        </h3>
        {spec.rels.length > 1 && (
          <Field label="Relationship">
            <select className={inputClass} value={e.rel} onChange={(ev) => p.onEdge(e.id, { rel: ev.target.value })}>
              {spec.rels.map((r) => (
                <option key={r.value} value={r.value}>
                  {r.label}
                </option>
              ))}
            </select>
          </Field>
        )}
        {!(p.doc.kind === "usecase" && (e.rel === "include" || e.rel === "extend")) && (
          <Field label={p.doc.kind === "state" ? "Event / guard" : "Label"}>
            <input
              className={inputClass}
              aria-label="Edge label"
              value={e.label}
              onChange={(ev) => p.onEdge(e.id, { label: ev.target.value }, `elabel:${e.id}`)}
            />
          </Field>
        )}
        {spec.cards === "free" && (
          <div className="grid grid-cols-2 gap-2">
            <Field label={`At ${nameOf(e.source)}`}>
              <input className={inputClass} placeholder="1" value={e.sourceCard} onChange={(ev) => p.onEdge(e.id, { sourceCard: ev.target.value }, `sc:${e.id}`)} />
            </Field>
            <Field label={`At ${nameOf(e.target)}`}>
              <input className={inputClass} placeholder="0..*" value={e.targetCard} onChange={(ev) => p.onEdge(e.id, { targetCard: ev.target.value }, `tc:${e.id}`)} />
            </Field>
          </div>
        )}
        {spec.cards === "er" && (
          <>
            <Field label={`${nameOf(e.source)} side`}>
              <select className={inputClass} value={e.sourceCard} onChange={(ev) => p.onEdge(e.id, { sourceCard: ev.target.value })}>
                {ER_CARDS.map((c) => (
                  <option key={c.value} value={c.value}>
                    {c.label}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={`${nameOf(e.target)} side`}>
              <select className={inputClass} value={e.targetCard} onChange={(ev) => p.onEdge(e.id, { targetCard: ev.target.value })}>
                {ER_CARDS.map((c) => (
                  <option key={c.value} value={c.value}>
                    {c.label}
                  </option>
                ))}
              </select>
            </Field>
          </>
        )}
        <div className="flex gap-2">
          <Btn small onClick={() => p.onReverse(e.id)}>
            Reverse
          </Btn>
          <Btn small variant="danger-outline" onClick={p.onDelete}>
            Delete
          </Btn>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <h3 className="text-xs font-semibold">{spec.title}</h3>
      <Field label="Direction">
        <select className={inputClass} value={p.doc.direction} onChange={(e) => p.onDirection(e.target.value as Direction)}>
          {DIRECTIONS.map((d) => (
            <option key={d.value} value={d.value}>
              {d.label}
            </option>
          ))}
        </select>
      </Field>
      <ul className="text-[11px] text-muted list-disc pl-4 space-y-1">
        <li>Add shapes from the toolbar.</li>
        <li>Drag from a node's edge dot to another node to connect them.</li>
        <li>Double-click a label to rename it.</li>
        {spec.groupShape && <li>Drop a node onto a {KIND_SPECS[p.doc.kind].shapes.find((s) => s.value === spec.groupShape)?.label.toLowerCase()} to put it inside.</li>}
        <li>Select something to edit it here; Delete removes it.</li>
      </ul>
    </div>
  );
}
