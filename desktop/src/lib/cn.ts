import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** 合并 Tailwind 类名,自动去重冲突,返回最终 className */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
