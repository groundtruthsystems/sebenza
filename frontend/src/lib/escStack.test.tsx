import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEscLayer, type EscLayerKind } from "./escStack";

afterEach(cleanup);

function Layer({ kind, active, onClose }: { kind: EscLayerKind; active: boolean; onClose: () => void }) {
  useEscLayer(kind, active, onClose);
  return null;
}

// TS-03: Esc closes the innermost open layer: dialog, then chat, then full screen.
describe("useEscLayer", () => {
  it("closes dialog, then chat, then full screen, one per press", async () => {
    const user = userEvent.setup();
    const closed: string[] = [];
    const state = { dialog: true, chat: true, fullscreen: true };
    const close = (k: keyof typeof state) => () => {
      closed.push(k);
      state[k] = false;
      rerender();
    };
    const tree = () => (
      <>
        {/* Registered out of order: priority is by kind, not mount order. */}
        <Layer kind="fullscreen" active={state.fullscreen} onClose={close("fullscreen")} />
        <Layer kind="dialog" active={state.dialog} onClose={close("dialog")} />
        <Layer kind="chat" active={state.chat} onClose={close("chat")} />
      </>
    );
    const r = render(tree());
    const rerender = () => r.rerender(tree());
    await user.keyboard("{Escape}");
    expect(closed).toEqual(["dialog"]);
    await user.keyboard("{Escape}");
    expect(closed).toEqual(["dialog", "chat"]);
    await user.keyboard("{Escape}");
    expect(closed).toEqual(["dialog", "chat", "fullscreen"]);
    await user.keyboard("{Escape}");
    expect(closed).toHaveLength(3);
  });

  it("leaves Esc to an open native dialog", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <>
        <Layer kind="fullscreen" active onClose={onClose} />
        <dialog open>native</dialog>
      </>,
    );
    await user.keyboard("{Escape}");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("ignores inactive layers", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<Layer kind="chat" active={false} onClose={onClose} />);
    await user.keyboard("{Escape}");
    expect(onClose).not.toHaveBeenCalled();
  });
});
