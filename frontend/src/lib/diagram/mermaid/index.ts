import type { AnyDoc, DiagramKind } from "../model";
import { classFromMermaid, classToMermaid } from "./classDiagram";
import { erFromMermaid, erToMermaid } from "./er";
import { flowchartFromMermaid, flowchartToMermaid } from "./flowchart";
import { sequenceFromMermaid, sequenceToMermaid } from "./sequence";
import { stateFromMermaid, stateToMermaid } from "./state";
import { USECASE_MARKER, usecaseFromMermaid, usecaseToMermaid } from "./usecase";

/** What kind of diagram a source declares, from its first statement. */
export function detectKind(src: string): DiagramKind | null {
  const lines = src.split("\n").map((l) => l.trim());
  const head = lines.find((l) => l !== "" && !l.startsWith("%%")) ?? "";
  const word = head.split(/\s+/)[0];
  switch (word) {
    case "classDiagram":
      return "class";
    case "flowchart":
    case "graph":
      return lines.some((l) => new RegExp(`^%%\\s*sebenza:kind\\s+${USECASE_MARKER}\\b`).test(l))
        ? "usecase"
        : "flowchart";
    case "stateDiagram":
    case "stateDiagram-v2":
      return "state";
    case "erDiagram":
      return "er";
    case "sequenceDiagram":
      return "sequence";
    default:
      return null;
  }
}

export interface Parsed {
  kind: DiagramKind | null;
  /** Null when the source uses something the canvas cannot represent. */
  doc: AnyDoc | null;
}

export function parseDiagram(src: string): Parsed {
  const kind = detectKind(src);
  switch (kind) {
    case "class":
      return { kind, doc: classFromMermaid(src) };
    case "flowchart":
      return { kind, doc: flowchartFromMermaid(src)?.doc ?? null };
    case "usecase":
      return { kind, doc: usecaseFromMermaid(src) };
    case "state":
      return { kind, doc: stateFromMermaid(src) };
    case "er":
      return { kind, doc: erFromMermaid(src) };
    case "sequence":
      return { kind, doc: sequenceFromMermaid(src) };
    default:
      return { kind: null, doc: null };
  }
}

export function serializeDiagram(doc: AnyDoc): string {
  switch (doc.kind) {
    case "class":
      return classToMermaid(doc);
    case "flowchart":
      return flowchartToMermaid(doc);
    case "usecase":
      return usecaseToMermaid(doc);
    case "state":
      return stateToMermaid(doc);
    case "er":
      return erToMermaid(doc);
    case "sequence":
      return sequenceToMermaid(doc);
  }
}
