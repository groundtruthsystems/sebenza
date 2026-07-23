import type { ButtonHTMLAttributes, ReactNode } from "react";

export default function LinkBtn({
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { children: ReactNode }) {
  return (
    <button
      type="button"
      className="text-[11px] text-accent cursor-pointer bg-transparent border-none p-0 hover:underline disabled:opacity-50 disabled:cursor-not-allowed"
      {...rest}
    >
      {children}
    </button>
  );
}
