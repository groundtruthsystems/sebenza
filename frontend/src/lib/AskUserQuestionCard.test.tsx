import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import AskUserQuestionCard from "./AskUserQuestionCard";
import type { AskUserQuestionInput } from "./types";

const singleSelect: AskUserQuestionInput = {
  questions: [
    {
      question: "Do you prefer cats or dogs?",
      header: "Pet type",
      multiSelect: false,
      options: [
        { label: "Cats", description: "Independent." },
        { label: "Dogs", description: "Loyal." },
      ],
    },
  ],
};

const multiSelect: AskUserQuestionInput = {
  questions: [
    {
      question: "Which toppings?",
      header: "Toppings",
      multiSelect: true,
      options: [{ label: "Cheese" }, { label: "Olives" }],
    },
  ],
};

describe("AskUserQuestionCard", () => {
  afterEach(() => cleanup());

  it("renders the question, options and a custom input", () => {
    render(<AskUserQuestionCard input={singleSelect} disabled={false} onSubmit={vi.fn()} />);

    expect(screen.getByText("Do you prefer cats or dogs?")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Cats/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Dogs/ })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Custom answer…")).toBeInTheDocument();
  });

  it("auto-sends a single-select answer on click", async () => {
    const onSubmit = vi.fn();
    render(<AskUserQuestionCard input={singleSelect} disabled={false} onSubmit={onSubmit} />);

    fireEvent.click(screen.getByRole("button", { name: /Cats/ }));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith("Pet type: Cats");
  });

  it("auto-sends a typed custom answer on Enter", async () => {
    const onSubmit = vi.fn();
    render(<AskUserQuestionCard input={singleSelect} disabled={false} onSubmit={onSubmit} />);

    const input = screen.getByPlaceholderText("Custom answer…");
    fireEvent.input(input, { target: { value: "A goldfish" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onSubmit).toHaveBeenCalledWith("Pet type: A goldfish");
  });

  it("uses a submit button for multi-select and joins selections", async () => {
    const onSubmit = vi.fn();
    render(<AskUserQuestionCard input={multiSelect} disabled={false} onSubmit={onSubmit} />);

    fireEvent.click(screen.getByRole("button", { name: "Cheese" }));
    fireEvent.click(screen.getByRole("button", { name: "Olives" }));
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Submit answer" }));
    expect(onSubmit).toHaveBeenCalledWith("Toppings: Cheese, Olives");
  });

  it("does not submit when disabled", async () => {
    const onSubmit = vi.fn();
    render(<AskUserQuestionCard input={singleSelect} disabled={true} onSubmit={onSubmit} />);

    fireEvent.click(screen.getByRole("button", { name: /Cats/ }));

    expect(onSubmit).not.toHaveBeenCalled();
  });
});
