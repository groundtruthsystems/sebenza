import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", () => ({
  api: {
    dismissNotification: vi.fn(() => Promise.resolve()),
  },
}));

import NotificationItem from "./NotificationItem";
import ToastStack from "./ToastStack";
import { api } from "./api";
import { useStore } from "../store";
import type { AppNotification } from "./types";

function resetStore(): void {
  useStore.setState({
    notifications: [],
    uiToasts: [],
    notificationHistory: [],
    unreadCount: 0,
  });
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  resetStore();
});

function createNotification(
  overrides: Partial<AppNotification> = {},
): AppNotification {
  return {
    id: 1,
    branch: "feature/toast-sizing",
    type: "runtime_error",
    message: "Notification text",
    url: "https://example.com/notifications/1",
    timestamp: Date.UTC(2026, 2, 24, 10, 30, 0),
    ...overrides,
  };
}

describe("ToastStack", () => {
  it("uses content-fit sizing with a capped max width", () => {
    useStore.setState({ notifications: [createNotification()] });

    render(<ToastStack onselect={vi.fn()} />);

    const alert = screen.getByRole("alert");
    const stack = alert.parentElement;

    expect(stack).not.toBeNull();
    expect(stack?.className).toContain("items-end");
    expect(alert.className).toContain("w-fit");
    expect(alert.className).toContain("max-w-[min(48ch,calc(100vw-2rem))]");
  });

  it("wraps toast content instead of truncating it", () => {
    const message =
      "This is a very long notification message that should wrap inside the toast instead of being truncated";
    const detail = "https://example.com/notifications/very/long/path/that/should/wrap";

    useStore.setState({ notifications: [createNotification({ message, url: detail })] });

    render(<ToastStack onselect={vi.fn()} />);

    const messageNode = screen.getByText(message);
    const detailNode = screen.getByText(detail);

    expect(messageNode.className).toContain("whitespace-normal");
    expect(messageNode.className).toContain("break-words");
    expect(messageNode.className).not.toContain("truncate");
    expect(detailNode.className).toContain("whitespace-normal");
    expect(detailNode.className).toContain("break-all");
    expect(detailNode.className).not.toContain("truncate");
  });

  it("keeps actionable toasts clickable and dismissible", async () => {
    const onselect = vi.fn();

    useStore.setState({ notifications: [createNotification()] });

    render(<ToastStack onselect={onselect} />);

    const selectButton = screen.getByRole("button", { name: /notification text/i });
    const dismissButton = screen
      .getAllByRole("button")
      .find((button) => button.textContent === "×");
    expect(dismissButton).toBeDefined();

    fireEvent.click(selectButton);
    fireEvent.click(dismissButton!);

    expect(onselect).toHaveBeenCalledWith("notification:1");
    // Dismiss routing moved into the store: the notification is removed locally
    // and the backend is told about it.
    expect(useStore.getState().notifications).toEqual([]);
    expect(api.dismissNotification).toHaveBeenCalledWith({ params: { id: 1 } });
  });
});

describe("NotificationItem", () => {
  it("keeps non-toast notification rows truncated by default", () => {
    const message = "Default notification item keeps truncation";
    const url = "https://example.com/default/truncation";

    render(<NotificationItem notification={createNotification({ message, url })} />);

    const messageNode = screen.getByText(message);
    const urlNode = screen.getByText(url);

    expect(messageNode.className).toContain("truncate");
    expect(urlNode.className).toContain("truncate");
  });
});
