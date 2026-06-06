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
  // 现代胶囊按钮:全圆角 + 主色按钮带轻微阴影，鼠标悬停时阴影/亮度同步增强;
  // 红色危险按钮颜色不变，仅形状统一为胶囊。
  const variants: Record<string, string> = {
    primary:
      "bg-primary text-primary-fg shadow-sm shadow-primary/30 " +
      "hover:brightness-110 hover:shadow-primary/40 active:scale-[0.98] disabled:opacity-50",
    secondary:
      "border border-border bg-bg-elev hover:bg-border/40 active:scale-[0.98] disabled:opacity-50",
    ghost: "hover:bg-border/40 disabled:opacity-50",
    danger:
      "bg-danger text-white shadow-sm shadow-danger/30 " +
      "hover:brightness-110 hover:shadow-danger/40 active:scale-[0.98] disabled:opacity-50",
  };
  const sizes: Record<string, string> = {
    sm: "h-8 px-4 text-xs rounded-full gap-1.5",
    md: "h-9 px-5 text-sm rounded-full gap-2",
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
        "h-9 w-full px-4 rounded-full border border-border bg-bg text-sm",
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

/** 自定义下拉:菜单与触发器都由前端绘制，展开/收起状态可控，箭头跟着旋转 */
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
          "h-9 w-full px-4 pr-10 rounded-full border border-border bg-bg text-sm text-left",
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
            "absolute right-3 top-1/2 -translate-y-1/2 text-fg-muted transition-transform",
            open && "rotate-180"
          )}
        />
      </button>
      {open && (
        <div
          className={cn(
            // 弹层宽度自适应:至少与触发器同宽，但允许超出以容纳长选项，避免被截断
            "absolute z-50 mt-1.5 min-w-full w-max max-w-[min(28rem,calc(100vw-2rem))]",
            "rounded-lg border border-border bg-bg-elev shadow-lg",
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
                  "w-full text-left px-3 py-1.5 text-sm flex items-center justify-between gap-2 whitespace-nowrap",
                  active ? "text-primary bg-primary/5" : "hover:bg-border/40"
                )}
              >
                <span>{o.label}</span>
                {active && <Check size={14} className="shrink-0" />}
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
  children?: React.ReactNode;
  desc?: string;
  right?: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 mb-3 min-w-0">
      <div className="min-w-0 flex-1">
        {children && (
          <div className="text-base font-semibold truncate">{children}</div>
        )}
        {desc && (
          <div className={`text-xs text-fg-muted truncate${children ? " mt-0.5" : ""}`}>{desc}</div>
        )}
      </div>
      {right && <div className="shrink-0">{right}</div>}
    </div>
  );
}
