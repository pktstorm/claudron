import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * Compose class names, letting later Tailwind utilities win over earlier ones.
 *
 * `clsx` handles conditionals; `twMerge` resolves conflicts. Plain string
 * concatenation cannot: `"bg-neutral-800" + " bg-sky-500/20"` leaves both in
 * the class list and the winner is decided by stylesheet order, not by the
 * caller. Every component here takes a `className` that is expected to override
 * its defaults, which is exactly that case.
 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
