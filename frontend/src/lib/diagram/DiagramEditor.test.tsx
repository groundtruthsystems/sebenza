import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import DiagramEditor from "./DiagramEditor";

const source = () => (screen.getByLabelText("Mermaid source") as HTMLTextAreaElement).value;

async function openSource(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("tab", { name: "source" }));
}

describe("DiagramEditor", () => {
  it("creates a class diagram from the palette and inserts its Mermaid", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<DiagramEditor initialSource={null} onSave={onSave} onCancel={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /Class diagram/ }));
    await user.click(screen.getByRole("button", { name: "+ Class" }));
    await user.click(screen.getByRole("button", { name: "+ Interface" }));

    await openSource(user);
    expect(source()).toMatch(/^classDiagram\n  class Class\n  class Interface \{\n    <<interface>>\n  \}/);

    await user.click(screen.getByRole("button", { name: "Insert diagram" }));
    expect(onSave).toHaveBeenCalledWith(expect.stringContaining("<<interface>>"));
  });

  it("undoes and redoes canvas edits", async () => {
    const user = userEvent.setup();
    render(<DiagramEditor initialSource={null} onSave={vi.fn()} onCancel={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Flowchart/ }));
    await user.click(screen.getByRole("button", { name: "+ Process" }));
    await user.click(screen.getByRole("button", { name: "+ Decision" }));
    await user.click(screen.getByRole("button", { name: "Undo" }));

    await openSource(user);
    expect(source()).toContain('n1["Step"]');
    expect(source()).not.toContain("Decision?");

    await user.click(screen.getByRole("button", { name: "Redo" }));
    expect(source()).toContain('{"Decision?"}');
  });

  it("saves an unchanged diagram byte for byte", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    const original = "classDiagram\n    Animal <|-- Duck\n    Duck : +swim()";
    render(<DiagramEditor initialSource={original} onSave={onSave} onCancel={vi.fn()} />);
    expect(screen.getByRole("tab", { name: "canvas" })).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("button", { name: "Update diagram" }));
    expect(onSave).toHaveBeenCalledWith(original);
  });

  it("keeps a diagram the canvas cannot represent as source, untouched", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    const original = "flowchart LR\n  A --> B\n  classDef hot fill:#f96\n  class A hot";
    render(<DiagramEditor initialSource={original} onSave={onSave} onCancel={vi.fn()} />);

    expect(screen.getByRole("tab", { name: "canvas" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("saved exactly as written");
    await user.click(screen.getByRole("button", { name: "Update diagram" }));
    expect(onSave).toHaveBeenCalledWith(original);
  });

  it("opens the canvas once typed source becomes representable", async () => {
    const user = userEvent.setup();
    render(<DiagramEditor initialSource={null} onSave={vi.fn()} onCancel={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Mermaid source/ }));
    expect(screen.getByRole("tab", { name: "canvas" })).toBeDisabled();
    await user.type(screen.getByLabelText("Mermaid source"), "erDiagram\n  A ||--o{{ B : has");
    expect(screen.getByRole("tab", { name: "canvas" })).toBeEnabled();
  });

  it("builds a sequence diagram from the toolbar", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<DiagramEditor initialSource={null} onSave={onSave} onCancel={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Sequence diagram/ }));
    await user.click(screen.getByRole("button", { name: "+ Actor" }));
    await user.click(screen.getByRole("button", { name: "+ Participant" }));
    await user.click(screen.getByRole("button", { name: "+ Message" }));
    await user.clear(screen.getByLabelText("Message text"));
    await user.type(screen.getByLabelText("Message text"), "save draft");
    await user.click(screen.getByRole("button", { name: "alt" }));

    await user.click(screen.getByRole("button", { name: "Insert diagram" }));
    expect(onSave.mock.calls[0][0]).toBe(
      ["sequenceDiagram", "  actor User", "  participant Service", "  alt condition", "    User->>Service: save draft", "  end"].join("\n"),
    );
  });

  it("asks before discarding changes", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    render(<DiagramEditor initialSource={null} onSave={vi.fn()} onCancel={onCancel} />);
    await user.click(screen.getByRole("button", { name: /State machine/ }));
    await user.click(screen.getByRole("button", { name: "+ State" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(onCancel).toHaveBeenCalled();
  });

  it("edits the selected node in the inspector", async () => {
    const user = userEvent.setup();
    render(<DiagramEditor initialSource={null} onSave={vi.fn()} onCancel={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Class diagram/ }));
    await user.click(screen.getByRole("button", { name: "+ Class" }));
    // A new node is selected, so the inspector shows it.
    const name = screen.getByLabelText("Name");
    await user.clear(name);
    await user.type(name, "Order line{Enter}");
    await user.type(screen.getAllByRole("textbox").find((t) => t.getAttribute("placeholder") === "+String name")!, "+int qty");
    await openSource(user);
    expect(source()).toContain("class Order_line {\n    +int qty\n  }");
  });
});
