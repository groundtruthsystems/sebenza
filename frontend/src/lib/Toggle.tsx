import type { MouseEvent } from "react";

export default function Toggle({
  checked = false,
  id,
  disabled = false,
  size = "default",
  preventMouseFocus = false,
  onToggle,
  "aria-label": ariaLabel,
}: {
  checked: boolean;
  id?: string;
  disabled?: boolean;
  size?: "default" | "sm";
  preventMouseFocus?: boolean;
  onToggle?: (checked: boolean) => void;
  "aria-label"?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      id={id}
      disabled={disabled}
      onMouseDown={(event: MouseEvent) => {
        if (!preventMouseFocus) return;
        event.preventDefault();
      }}
      onClick={() => onToggle?.(!checked)}
      className={`toggle${checked ? " on" : ""}${size === "sm" ? " sm" : ""}`}
    >
      <span className="knob" />
    </button>
  );
}
