import * as TogglePrimitive from "@radix-ui/react-toggle";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

/**
 * A two-state control that reports its state as `aria-pressed`.
 *
 * Used for filter chips, which select and deselect rather than fire an action.
 * The plain `<button onClick={() => set(x === v ? null : v)}>` this replaced
 * behaved the same way but told assistive technology nothing about it, and gave
 * tests no way to observe the selected state except by reading class names.
 *
 * Variants carry only theme-role colours. Application meanings -- sky for
 * liveness, purple for manual status -- are passed in as `className` and win,
 * because `cn` resolves Tailwind conflicts in favour of the caller.
 */
const toggleVariants = cva(
  "inline-flex items-center justify-center rounded transition-colors outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        default: "bg-secondary text-muted-foreground hover:text-foreground",
      },
      size: {
        sm: "px-2 py-0.5 text-[11px]",
        default: "px-2 py-1 text-xs",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Toggle({
  className,
  variant,
  size,
  ...props
}: React.ComponentProps<typeof TogglePrimitive.Root> & VariantProps<typeof toggleVariants>) {
  return (
    <TogglePrimitive.Root
      data-slot="toggle"
      className={cn(toggleVariants({ variant, size }), className)}
      {...props}
    />
  );
}

// Only `Toggle` is exported. shadcn ships `toggleVariants` as a public export
// too, but nothing consumes it here, and exporting a non-component alongside a
// component breaks Fast Refresh for the file. Export it when something needs it.
export { Toggle };
