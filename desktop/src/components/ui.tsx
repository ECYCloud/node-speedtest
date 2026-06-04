import { ButtonHTMLAttributes, InputHTMLAttributes, useEffect, useRef, useState } from "react";
import { ChevronDown, Check } from "lucide-react";
import { cn } from "../lib/cn";

export function Card({
  className,
  children,
}: {
  className?: string;
  children?: React.ReactNode;
}) {
  return (
    <div
      className={cn(
        "rounded-2xl border border-border bg-bg-elev shadow-sm",
        className
      )}
    >
      {children}
    </div>
  );
}

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
}

export function Button({
  variant = "secondary",
  size = "md",
  className,
  ...rest
}: ButtonProps) {
  const variants: Record<string, string> = {
    primary:
      "bg-primary text-primary-fg hover:brightness-110 active:scale-[0.98] disabled:opacity-50",
    secondary:
      "border border-border bg-bg-elev hover:bg-border/40 active:scale-[0.98] disabled:opacity-50",
    ghost: "hover:bg-border/40 disabled:opacity-50",
    danger:
      "bg-danger text-white hover:brightness-110 active:scale-[0.98] disabled:opacity-50",
  };
  const sizes: Record<string, string> = {
    sm: "h-8 px-3 text-xs rounded-md gap-1.5",
    md: "h-9 px-3.5 text-sm rounded-lg gap-2",
  };
  return (
    <button
      className={cn(
        "inline-flex items-center justify-center font-medium transition select-none",
        variants[variant],
        sizes[size],
        className
      )}
      {...rest}
    />
  );
}

export function Input({
  className,
  ...rest
}: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        "h-9 w-full px-3 rounded-lg border border-border bg-bg text-sm",
        "outline-none focus:border-primary focus:ring-2 focus:ring-primary/20",
        "placeholder:text-fg-muted/70",
        className
      )}
      {...rest}
    />
  );
}

export interface SelectOption {
  value: string;
  label: string;
}

interface SelectProps {
  value: string;
  onChange: (v: string) => void;
  options: SelectOption[];
  disabled?: boolean;
  className?: string;
  placeholder?: string;
}

/** 自定义下拉:菜单与触发器都由前端绘制,展开/收起状态可控,箭头跟着旋转 */
export function Select({
  value,
  onChange,
  options,
  disabled,
  className,
  placeholder = "请选择",
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const cur = options.find((o) => o.value === value);

  return (
    <div ref={ref} className={cn("relative", className)}>
      <button
        type="button"
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "h-9 w-full px-3 pr-9 rounded-lg border border-border bg-bg text-sm text-left",
          "outline-none focus:border-primary focus:ring-2 focus:ring-primary/20",
          "disabled:opacity-50 flex items-center"
        )}
      >
        <span className={cn("truncate", !cur && "text-fg-muted/70")}>
          {cur?.label ?? placeholder}
        </span>
        <ChevronDown
          size={16}
          className={cn(
            "absolute right-2.5 top-1/2 -translate-y-1/2 text-fg-muted transition-transform",
            open && "rotate-180"
          )}
        />
      </button>
      {open && (
        <div
          className={cn(
            "absolute z-50 mt-1.5 w-full rounded-lg border border-border bg-bg-elev shadow-lg",
            "max-h-60 overflow-auto py-1"
          )}
        >
          {options.map((o) => {
            const active = o.value === value;
            return (
              <button
                key={o.value}
                type="button"
                onClick={() => {
                  onChange(o.value);
                  setOpen(false);
                }}
                className={cn(
                  "w-full text-left px-3 py-1.5 text-sm flex items-center justify-between gap-2",
                  active ? "text-primary bg-primary/5" : "hover:bg-border/40"
                )}
              >
                <span className="truncate">{o.label}</span>
                {active && <Check size={14} />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

export function Badge({
  children,
  variant = "neutral",
  className,
}: {
  children: React.ReactNode;
  variant?: "neutral" | "success" | "warning" | "danger" | "primary";
  className?: string;
}) {
  const map: Record<string, string> = {
    neutral: "bg-border/40 text-fg-muted",
    success: "bg-success/15 text-success",
    warning: "bg-warning/20 text-warning",
    danger: "bg-danger/15 text-danger",
    primary: "bg-primary/15 text-primary",
  };
  return (
    <span
      className={cn(
        "inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium",
        map[variant],
        className
      )}
    >
      {children}
    </span>
  );
}

export function SectionTitle({
  children,
  desc,
  right,
}: {
  children: React.ReactNode;
  desc?: string;
  right?: React.ReactNode;
}) {
  return (
    <div className="flex items-end justify-between gap-3 mb-3 min-w-0">
      <div className="min-w-0 flex-1">
        <div className="text-base font-semibold truncate">{children}</div>
        {desc && (
          <div className="text-xs text-fg-muted mt-0.5 truncate">{desc}</div>
        )}
      </div>
      {right && <div className="shrink-0">{right}</div>}
    </div>
  );
}
