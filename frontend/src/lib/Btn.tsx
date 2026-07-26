import type { ButtonHTMLAttributes, ReactNode } from "react";

type Variant = "default" | "cta" | "danger" | "accent-outline" | "danger-outline";

const base = "rounded-md border cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed";

const sizes: Record<string, string> = {
  normal: "px-3 py-1.5 text-xs",
  small: "px-2.5 py-1 text-[11px] font-semibold",
};

const variants: Record<Variant, string> = {
  default: "border-edge bg-surface text-primary hover:bg-hover",
  cta: "border-accent bg-accent text-white hover:opacity-90",
  danger: "border-danger bg-danger text-white hover:opacity-90",
  "accent-outline": "border-accent text-accent bg-surface hover:bg-accent/10",
  "danger-outline": "border-danger text-danger bg-surface hover:bg-danger/10",
};

export default function Btn({
  variant = "default",
  small = false,
  className = "",
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  small?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      className={`${base} ${sizes[small ? "small" : "normal"]} ${variants[variant]} ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}
