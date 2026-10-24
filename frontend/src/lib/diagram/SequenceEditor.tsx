import { useMemo, useRef, useState, type PointerEvent } from "react";
import Btn from "../Btn";
import { Field, inputClass } from "./Inspector";
import type { SeqArrow, SeqBlock, SeqItem, SequenceDoc } from "./model";
import { BRANCH_WORD, SEQ_ARROWS, SEQ_BLOCKS } from "./mermaid/sequence";
import type { ChangeOpts } from "./GraphCanvas";
import {
  addParticipant,
  canBranch,
  canWrap,
  deleteRows,
  depths,
  insertItems,
  matchBlocks,
  moveParticipant,
  moveRow,
  removeParticipant,
  renameParticipant,
  updateItem,
  wrapRows,
} from "./seqOps";

/**
 * Sequence diagrams get their own lane view rather than the node canvas:
 * participants are columns, rows run top to bottom in order, and a message is
 * drawn by dragging from one lifeline to another.
 */

const COL = 170;
const LEFT = 30;
const HEAD = 64;
const ROW = 40;

interface Props {
  doc: SequenceDoc;
  onChange(doc: SequenceDoc, opts?: ChangeOpts): void;
}

type Selection =
  | { type: "participant"; id: string }
  | { type: "rows"; anchor: number; focus: number }
  | null;

export default function SequenceEditor({ doc, onChange }: Props) {
  const [sel, setSel] = useState<Selection>(null);
  const [drag, setDrag] = useState<{ from: string; x0: number; x: number; y: number } | null>(null);
  const svg = useRef<SVGSVGElement>(null);

  const colX = (id: string) => LEFT + COL / 2 + doc.participants.findIndex((p) => p.id === id) * COL;
  const rowY = (i: number) => HEAD + 28 + i * ROW;
  const width = Math.max(LEFT * 2 + doc.participants.length * COL, 480);
  // Room below the last row, so there is always lifeline to drag a new
  // message along - an empty diagram would otherwise be a header only.
  const height = Math.max(rowY(doc.items.length) + ROW * 3, 360);
  const levels = useMemo(() => depths(doc.items), [doc.items]);
  const pairs = useMemo(() => matchBlocks(doc.items), [doc.items]);

  const range = sel?.type === "rows" ? [Math.min(sel.anchor, sel.focus), Math.max(sel.anchor, sel.focus)] : null;
  const isSelected = (i: number) => !!range && i >= range[0] && i <= range[1];
  /** New rows go after the selection, or at the end. */
  const insertAt = range ? range[1] + 1 : doc.items.length;

  const insert = (items: SeqItem[]) => {
    onChange(insertItems(doc, insertAt, items));
    setSel({ type: "rows", anchor: insertAt, focus: insertAt + items.length - 1 });
  };

  const addMessage = (from?: string, to?: string) => {
    const [a, b] = doc.participants;
    if (!a) return;
    insert([{ t: "msg", from: from ?? a.id, to: to ?? (b ?? a).id, arrow: "->>", text: "message" }]);
  };

  const columnAt = (x: number): string | null => {
    const i = Math.floor((x - LEFT) / COL);
    return doc.participants[i]?.id ?? null;
  };
  const local = (e: PointerEvent) => {
    const r = svg.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerUp = (e: PointerEvent<SVGSVGElement>) => {
    if (!drag) return;
    const to = columnAt(local(e).x);
    setDrag(null);
    if (to && Math.abs(local(e).x - drag.x0) > 8) addMessage(drag.from, to);
    else if (to === drag.from) addMessage(drag.from, drag.from);
  };

  const row = (i: number, e: { shiftKey: boolean }) => {
    if (e.shiftKey && sel?.type === "rows") setSel({ ...sel, focus: i });
    else setSel({ type: "rows", anchor: i, focus: i });
  };

  return (
    <div className="flex-1 min-h-0 flex">
      <div className="flex-1 min-w-0 flex flex-col">
        <div className="flex flex-wrap items-center gap-1.5 px-3 py-2 border-b border-edge bg-sidebar" role="toolbar" aria-label="Diagram tools">
          <Btn small onClick={() => setSel({ type: "participant", id: addP(doc, "participant", onChange) })}>
            + Participant
          </Btn>
          <Btn small onClick={() => setSel({ type: "participant", id: addP(doc, "actor", onChange) })}>
            + Actor
          </Btn>
          <span className="mx-1 h-4 w-px bg-edge" />
          <Btn small disabled={!doc.participants.length} onClick={() => addMessage()}>
            + Message
          </Btn>
          <Btn
            small
            disabled={!doc.participants.length}
            onClick={() => insert([{ t: "note", pos: "over", who: [doc.participants[0].id], text: "note" }])}
          >
            + Note
          </Btn>
          <span className="mx-1 h-4 w-px bg-edge" />
          <span className="text-[11px] text-muted">Wrap in</span>
          {SEQ_BLOCKS.map((b) => (
            <Btn
              key={b}
              small
              disabled={!range || !canWrap(doc.items, range[0], range[1])}
              title={range ? "Wrap the selected rows" : "Select rows first (shift-click for a range)"}
              onClick={() => {
                if (!range) return;
                onChange(wrapRows(doc, range[0], range[1], b));
                setSel({ type: "rows", anchor: range[0], focus: range[0] });
              }}
            >
              {b}
            </Btn>
          ))}
          <Btn
            small
            disabled={!range || !canBranch(doc.items, range[1])}
            title="Add another branch (else / and / option)"
            onClick={() => insert([{ t: "else", text: "otherwise" }])}
          >
            + Branch
          </Btn>
          <span className="mx-1 h-4 w-px bg-edge" />
          <label className="flex items-center gap-1 text-[11px] text-muted">
            <input type="checkbox" checked={doc.autonumber} onChange={(e) => onChange({ ...doc, autonumber: e.target.checked })} />
            Number messages
          </label>
        </div>
        <div className="flex-1 min-h-0 overflow-auto diagram-canvas" onClick={(e) => e.target === e.currentTarget && setSel(null)}>
          {doc.participants.length === 0 ? (
            <p className="p-6 text-xs text-muted">Add a participant or actor to start.</p>
          ) : (
            <svg
              ref={svg}
              width={width}
              height={height}
              className="select-none"
              onPointerMove={(e) => drag && setDrag({ ...drag, ...local(e) })}
              onPointerUp={onPointerUp}
              onPointerLeave={() => setDrag(null)}
              role="img"
              aria-label="Sequence diagram"
            >
              <defs>
                <marker id="sq-fill" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="10" markerHeight="10" orient="auto-start-reverse">
                  <path d="M0,0 L10,5 L0,10 Z" fill="var(--diagram-stroke)" />
                </marker>
                <marker id="sq-open" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="10" markerHeight="10" orient="auto-start-reverse">
                  <path d="M0,0 L10,5 L0,10" fill="none" stroke="var(--diagram-stroke)" strokeWidth="1.5" />
                </marker>
                <marker id="sq-x" viewBox="0 0 10 10" refX="5" refY="5" markerWidth="10" markerHeight="10">
                  <path d="M1,1 L9,9 M9,1 L1,9" stroke="var(--diagram-stroke)" strokeWidth="1.8" />
                </marker>
              </defs>

              {/* Row hit areas sit beneath everything else. */}
              {doc.items.map((_, i) => (
                <rect
                  key={`hit-${i}`}
                  x={0}
                  y={rowY(i) - ROW / 2}
                  width={width}
                  height={ROW}
                  className={isSelected(i) ? "fill-[var(--color-active)]" : "fill-transparent hover:fill-[var(--color-hover)]"}
                  onClick={(e) => row(i, e)}
                  data-row={i}
                />
              ))}

              {/* Block frames. */}
              {doc.items.map((it, i) => {
                if (it.t !== "start") return null;
                const end = pairs.get(i) ?? doc.items.length;
                const inset = 6 + levels[i] * 8;
                return (
                  <g key={`frame-${i}`} pointerEvents="none">
                    <rect
                      x={inset}
                      y={rowY(i) - ROW / 2 + 4}
                      width={width - inset * 2}
                      height={rowY(end) - rowY(i)}
                      fill="none"
                      stroke="var(--diagram-line)"
                      rx={3}
                    />
                  </g>
                );
              })}

              {/* Lifelines, with a wide invisible strip to drag messages from. */}
              {doc.participants.map((p) => {
                const x = colX(p.id);
                return (
                  <g key={`life-${p.id}`}>
                    <line x1={x} x2={x} y1={HEAD} y2={height - 8} stroke="var(--diagram-line)" strokeDasharray="4 4" pointerEvents="none" />
                    <rect
                      x={x - 10}
                      y={HEAD}
                      width={20}
                      height={height - HEAD}
                      className="fill-transparent cursor-crosshair hover:fill-[var(--color-active)]"
                      onPointerDown={(e) => {
                        e.preventDefault();
                        const pt = local(e);
                        setDrag({ from: p.id, x0: pt.x, ...pt });
                      }}
                    >
                      <title>Drag to another lifeline to add a message</title>
                    </rect>
                  </g>
                );
              })}

              {/* Participant heads. */}
              {doc.participants.map((p) => {
                const x = colX(p.id);
                const chosen = sel?.type === "participant" && sel.id === p.id;
                return (
                  <g key={`head-${p.id}`} onClick={() => setSel({ type: "participant", id: p.id })} className="cursor-pointer" data-participant={p.id}>
                    {p.kind === "actor" ? (
                      <g transform={`translate(${x - 9}, 6)`} stroke={chosen ? "var(--color-accent)" : "var(--diagram-stroke)"} fill="none" strokeWidth="1.5">
                        <circle cx="9" cy="5" r="5" />
                        <path d="M9 10 V22 M1 14 H17 M9 22 L2 32 M9 22 L16 32" />
                      </g>
                    ) : (
                      <rect
                        x={x - COL / 2 + 12}
                        y={10}
                        width={COL - 24}
                        height={HEAD - 30}
                        rx={4}
                        fill="var(--color-sidebar)"
                        stroke={chosen ? "var(--color-accent)" : "var(--diagram-stroke)"}
                      />
                    )}
                    <text x={x} y={p.kind === "actor" ? HEAD - 14 : 10 + (HEAD - 30) / 2 + 4} textAnchor="middle" className="fill-primary text-[12px]">
                      {p.label}
                    </text>
                  </g>
                );
              })}

              {/* Rows. */}
              {doc.items.map((it, i) => (
                <g key={`row-${i}`} pointerEvents="none">
                  <RowGlyph item={it} y={rowY(i)} colX={colX} width={width} inset={6 + levels[i] * 8} enclosing={it.t === "else" ? enclosingWord(doc.items, i) : ""} />
                </g>
              ))}

              {drag && (
                <line x1={colX(drag.from)} y1={drag.y} x2={drag.x} y2={drag.y} stroke="var(--color-accent)" strokeDasharray="4 3" markerEnd="url(#sq-fill)" pointerEvents="none" />
              )}
            </svg>
          )}
        </div>
      </div>
      <aside className="w-64 shrink-0 border-l border-edge bg-sidebar p-3 overflow-y-auto" aria-label="Inspector">
        <SequenceInspector doc={doc} sel={sel} setSel={setSel} onChange={onChange} />
      </aside>
    </div>
  );
}

function addP(doc: SequenceDoc, kind: "participant" | "actor", onChange: Props["onChange"]): string {
  const r = addParticipant(doc, kind);
  onChange(r.doc);
  return r.id;
}

function enclosingWord(items: SeqItem[], i: number): string {
  let level = 0;
  for (let k = i - 1; k >= 0; k--) {
    const it = items[k];
    if (it.t === "end") level++;
    if (it.t === "start") {
      if (level === 0) return BRANCH_WORD[it.block] ?? "else";
      level--;
    }
  }
  return "else";
}

function RowGlyph({
  item,
  y,
  colX,
  width,
  inset,
  enclosing,
}: {
  item: SeqItem;
  y: number;
  colX: (id: string) => number;
  width: number;
  inset: number;
  enclosing: string;
}) {
  const text = "fill-primary text-[11px]";
  switch (item.t) {
    case "msg": {
      const x1 = colX(item.from);
      const x2 = colX(item.to);
      const dashed = item.arrow.startsWith("--");
      const marker = item.arrow.endsWith("x") ? "sq-x" : item.arrow.endsWith(")") ? "sq-open" : item.arrow.endsWith(">>") ? "sq-fill" : undefined;
      const style = { stroke: "var(--diagram-stroke)", strokeWidth: 1.5, strokeDasharray: dashed ? "5 4" : undefined, fill: "none" };
      if (x1 === x2) {
        return (
          <>
            <path d={`M${x1},${y - 8} h40 v16 h-38`} style={style} markerEnd={marker ? `url(#${marker})` : undefined} />
            <text x={x1 + 46} y={y + 4} className={text}>
              {item.text}
            </text>
          </>
        );
      }
      return (
        <>
          <line x1={x1} y1={y + 6} x2={x2 + (x2 > x1 ? -2 : 2)} y2={y + 6} style={style} markerEnd={marker ? `url(#${marker})` : undefined} />
          <text x={(x1 + x2) / 2} y={y - 2} textAnchor="middle" className={text}>
            {item.act === "+" ? "▸ " : item.act === "-" ? "◂ " : ""}
            {item.text}
          </text>
        </>
      );
    }
    case "note": {
      const xs = item.who.map(colX);
      const lo = Math.min(...xs) + (item.pos === "right of" ? 20 : item.pos === "left of" ? -150 : -60);
      const hi = Math.max(...xs) + (item.pos === "right of" ? 150 : item.pos === "left of" ? -20 : 60);
      return (
        <>
          <rect x={lo} y={y - 12} width={hi - lo} height={24} fill="#3a3416" stroke="#8d7a2a" rx={2} />
          <text x={(lo + hi) / 2} y={y + 4} textAnchor="middle" className={text}>
            {item.text}
          </text>
        </>
      );
    }
    case "start":
      return (
        <>
          <rect x={inset} y={y - 14} width={44} height={16} fill="var(--diagram-line)" />
          <text x={inset + 22} y={y - 2} textAnchor="middle" className="fill-primary text-[10px] font-semibold">
            {item.block}
          </text>
          <text x={inset + 52} y={y - 2} className="fill-muted text-[11px]">
            [{item.text}]
          </text>
        </>
      );
    case "else":
      return (
        <>
          <line x1={inset} x2={width - inset} y1={y - 6} y2={y - 6} stroke="var(--diagram-line)" strokeDasharray="6 4" />
          <text x={inset + 8} y={y + 10} className="fill-muted text-[11px]">
            {enclosing} [{item.text}]
          </text>
        </>
      );
    case "end":
      return (
        <text x={inset + 8} y={y + 4} className="fill-muted text-[10px]">
          end
        </text>
      );
    case "activate":
      return (
        <text x={colX(item.who) + 8} y={y + 4} className="fill-muted text-[10px]">
          {item.on ? "activate" : "deactivate"} {item.who}
        </text>
      );
  }
}

function SequenceInspector({
  doc,
  sel,
  setSel,
  onChange,
}: {
  doc: SequenceDoc;
  sel: Selection;
  setSel: (s: Selection) => void;
  onChange: Props["onChange"];
}) {
  const options = doc.participants.map((p) => (
    <option key={p.id} value={p.id}>
      {p.label}
    </option>
  ));

  if (sel?.type === "participant") {
    const p = doc.participants.find((x) => x.id === sel.id);
    if (!p) return null;
    return (
      <div className="flex flex-col gap-3">
        <h3 className="text-xs font-semibold">{p.kind === "actor" ? "Actor" : "Participant"}</h3>
        <Field label="Name">
          <input
            className={inputClass}
            aria-label="Participant name"
            value={p.label}
            onChange={(e) => {
              const r = renameParticipant(doc, p.id, e.target.value);
              setSel({ type: "participant", id: r.id });
              // Keyed by column, not id: the id follows the name as it is typed.
              onChange(r.doc, { coalesce: `pname:${doc.participants.indexOf(p)}` });
            }}
          />
        </Field>
        <Field label="Drawn as">
          <select
            className={inputClass}
            value={p.kind}
            onChange={(e) =>
              onChange({
                ...doc,
                participants: doc.participants.map((x) => (x.id === p.id ? { ...x, kind: e.target.value as "participant" | "actor" } : x)),
              })
            }
          >
            <option value="participant">Box</option>
            <option value="actor">Actor</option>
          </select>
        </Field>
        <div className="flex gap-2">
          <Btn small onClick={() => onChange(moveParticipant(doc, p.id, -1))}>
            ← Move
          </Btn>
          <Btn small onClick={() => onChange(moveParticipant(doc, p.id, 1))}>
            Move →
          </Btn>
        </div>
        <Btn
          small
          variant="danger-outline"
          onClick={() => {
            onChange(removeParticipant(doc, p.id));
            setSel(null);
          }}
        >
          Delete (and its messages)
        </Btn>
      </div>
    );
  }

  if (sel?.type === "rows") {
    const lo = Math.min(sel.anchor, sel.focus);
    const hi = Math.max(sel.anchor, sel.focus);
    const del = (
      <Btn
        small
        variant="danger-outline"
        onClick={() => {
          onChange(deleteRows(doc, lo, hi));
          setSel(null);
        }}
      >
        Delete
      </Btn>
    );
    if (lo !== hi) {
      return (
        <div className="flex flex-col gap-3">
          <h3 className="text-xs font-semibold">{hi - lo + 1} rows</h3>
          <p className="text-[11px] text-muted">Use “Wrap in” on the toolbar to put these in a block.</p>
          {del}
        </div>
      );
    }
    const it = doc.items[lo];
    if (!it) return null;
    const set = (patch: Partial<SeqItem>, coalesce?: string) => onChange(updateItem(doc, lo, patch), { coalesce });
    const move = (
      <div className="flex gap-2">
        <Btn small onClick={() => { onChange(moveRow(doc, lo, -1)); setSel({ type: "rows", anchor: lo - 1, focus: lo - 1 }); }} disabled={lo === 0}>
          ↑
        </Btn>
        <Btn small onClick={() => { onChange(moveRow(doc, lo, 1)); setSel({ type: "rows", anchor: lo + 1, focus: lo + 1 }); }} disabled={lo === doc.items.length - 1}>
          ↓
        </Btn>
        {del}
      </div>
    );
    switch (it.t) {
      case "msg":
        return (
          <div className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold">Message</h3>
            <div className="grid grid-cols-2 gap-2">
              <Field label="From">
                <select className={inputClass} value={it.from} onChange={(e) => set({ from: e.target.value })}>
                  {options}
                </select>
              </Field>
              <Field label="To">
                <select className={inputClass} value={it.to} onChange={(e) => set({ to: e.target.value })}>
                  {options}
                </select>
              </Field>
            </div>
            <Field label="Text">
              <input className={inputClass} aria-label="Message text" value={it.text} onChange={(e) => set({ text: e.target.value }, `mtext:${lo}`)} />
            </Field>
            <Field label="Kind">
              <select className={inputClass} value={it.arrow} onChange={(e) => set({ arrow: e.target.value as SeqArrow })}>
                {SEQ_ARROWS.map((a) => (
                  <option key={a.value} value={a.value}>
                    {a.label}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Activation">
              <select className={inputClass} value={it.act ?? ""} onChange={(e) => set({ act: (e.target.value || undefined) as "+" | "-" | undefined })}>
                <option value="">None</option>
                <option value="+">Activates the receiver</option>
                <option value="-">Ends the sender’s activation</option>
              </select>
            </Field>
            {move}
          </div>
        );
      case "note":
        return (
          <div className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold">Note</h3>
            <Field label="Position">
              <select className={inputClass} value={it.pos} onChange={(e) => set({ pos: e.target.value as "over" | "left of" | "right of" })}>
                <option value="over">over</option>
                <option value="left of">left of</option>
                <option value="right of">right of</option>
              </select>
            </Field>
            <Field label="Participant">
              <select className={inputClass} value={it.who[0]} onChange={(e) => set({ who: [e.target.value, ...it.who.slice(1)] })}>
                {options}
              </select>
            </Field>
            {it.pos === "over" && (
              <Field label="Spanning to">
                <select className={inputClass} value={it.who[1] ?? ""} onChange={(e) => set({ who: e.target.value ? [it.who[0], e.target.value] : [it.who[0]] })}>
                  <option value="">(just one)</option>
                  {options}
                </select>
              </Field>
            )}
            <Field label="Text">
              <input className={inputClass} aria-label="Note text" value={it.text} onChange={(e) => set({ text: e.target.value }, `ntext:${lo}`)} />
            </Field>
            {move}
          </div>
        );
      case "start":
        return (
          <div className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold">Block</h3>
            <Field label="Kind">
              <select className={inputClass} value={it.block} onChange={(e) => set({ block: e.target.value as SeqBlock })}>
                {SEQ_BLOCKS.map((b) => (
                  <option key={b} value={b}>
                    {b}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Condition / label">
              <input className={inputClass} aria-label="Block label" value={it.text} onChange={(e) => set({ text: e.target.value }, `btext:${lo}`)} />
            </Field>
            <p className="text-[11px] text-muted">Deleting a block keeps the rows inside it.</p>
            {del}
          </div>
        );
      case "else":
        return (
          <div className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold">Branch</h3>
            <Field label="Condition / label">
              <input className={inputClass} aria-label="Branch label" value={it.text} onChange={(e) => set({ text: e.target.value }, `etext:${lo}`)} />
            </Field>
            {del}
          </div>
        );
      case "end":
        return (
          <div className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold">End of block</h3>
            {del}
          </div>
        );
      case "activate":
        return (
          <div className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold">Activation</h3>
            <Field label="Participant">
              <select className={inputClass} value={it.who} onChange={(e) => set({ who: e.target.value })}>
                {options}
              </select>
            </Field>
            <Field label="Action">
              <select className={inputClass} value={it.on ? "on" : "off"} onChange={(e) => set({ on: e.target.value === "on" })}>
                <option value="on">activate</option>
                <option value="off">deactivate</option>
              </select>
            </Field>
            {move}
          </div>
        );
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <h3 className="text-xs font-semibold">Sequence diagram</h3>
      <ul className="text-[11px] text-muted list-disc pl-4 space-y-1">
        <li>Add participants and actors from the toolbar.</li>
        <li>Drag from one dashed lifeline to another to add a message.</li>
        <li>Click a row to edit it; shift-click to select a range, then wrap it in alt, loop, opt…</li>
        <li>New rows go after the selection.</li>
      </ul>
    </div>
  );
}
