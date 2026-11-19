import { beforeEach, describe, expect, it } from "vitest";
import {
  closeFullscreen,
  fullscreenUrl,
  inAppOrigin,
  openFullscreen,
  parseInboxRoute,
  setOriginProvider,
} from "./fullscreenRoute";

beforeEach(() => {
  window.history.replaceState(null, "", "/inbox");
  setOriginProvider(null);
});

describe("parseInboxRoute", () => {
  it("reads the item, full-screen flag, document and handover", () => {
    expect(parseInboxRoute("?item=req-a&view=full&doc=brief")).toEqual({
      item: "req-a",
      full: true,
      doc: "brief",
      handover: null,
    });
    expect(parseInboxRoute("?item=req-a&handover=acme&view=full")).toMatchObject({ handover: "acme", full: true });
  });

  it("is not full screen without an item", () => {
    expect(parseInboxRoute("?view=full").full).toBe(false);
    expect(parseInboxRoute("").item).toBeNull();
  });
});

describe("fullscreenUrl", () => {
  it("builds the request, document and handover URLs", () => {
    expect(fullscreenUrl("req-a")).toBe("/inbox?item=req-a&view=full");
    expect(fullscreenUrl("req-a", { doc: "n2" })).toBe("/inbox?item=req-a&view=full&doc=n2");
    expect(fullscreenUrl("req-a", { handover: "acme" })).toBe("/inbox?item=req-a&handover=acme&view=full");
  });
});

describe("openFullscreen and closeFullscreen", () => {
  it("pushes the origin route, selection and scroll, and Close goes back to it", () => {
    window.history.replaceState(null, "", "/inbox?item=req-b");
    setOriginProvider(() => ({ selection: "req-b", scroll: 120 }));
    const before = window.history.length;
    openFullscreen("req-b");
    expect(window.location.search).toBe("?item=req-b&view=full");
    expect(window.history.state.fullscreen).toEqual({ route: "/inbox?item=req-b", selection: "req-b", scroll: 120 });
    expect(inAppOrigin()).toEqual({ route: "/inbox?item=req-b", selection: "req-b", scroll: 120 });
    closeFullscreen("req-b");
    expect(window.location.search).toBe("?item=req-b");
    expect(window.history.length).toBe(before + 1);
  });

  it("after a direct load, Close replaces the entry with the request's own route", () => {
    window.history.pushState(null, "", "/inbox?item=req-a&view=full");
    const before = window.history.length;
    expect(inAppOrigin()).toBeNull();
    closeFullscreen("req-a");
    expect(window.location.search).toBe("?item=req-a");
    expect(window.history.length).toBe(before);
  });

  it("treats a state that survived a reload as a direct load", () => {
    const origin = { route: "/inbox", selection: null, scroll: 0 };
    window.history.pushState({ fullscreen: origin, fullscreenKey: "from-before-reload" }, "", "/inbox?item=req-a&view=full");
    expect(inAppOrigin()).toBeNull();
  });
});
