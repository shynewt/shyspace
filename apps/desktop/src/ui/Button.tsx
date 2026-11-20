import { motion } from "motion/react";
import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { cx } from "./cx";
import { springs } from "./motion";
import { Spinner } from "./Spinner";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md" | "lg";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-accent text-on-accent shadow-card hover:bg-accent-strong",
  secondary: "border border-line bg-surface text-ink hover:border-surface-3 hover:bg-surface-2",
  ghost: "text-muted hover:bg-surface-2 hover:text-ink",
  danger: "bg-danger text-white shadow-card hover:brightness-110",
};

const SIZES: Record<Size, string> = {
  sm: "h-7 gap-1.5 rounded-md px-2.5 text-[12.5px]",
  md: "h-8 gap-2 rounded-lg px-3 text-[13px]",
  lg: "h-10 gap-2 rounded-lg px-4 text-[14px]",
};

type ButtonProps = Omit<ComponentPropsWithoutRef<typeof motion.button>, "children"> & {
  variant?: Variant;
  size?: Size;
  icon?: ReactNode;
  isLoading?: boolean;
  children?: ReactNode;
};

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  isLoading = false,
  disabled,
  className,
  children,
  type = "button",
  ...props
}: ButtonProps) {
  const isDisabled = disabled || isLoading;
  return (
    <motion.button
      type={type}
      disabled={isDisabled}
      whileTap={isDisabled ? undefined : { scale: 0.97 }}
      transition={springs.snappy}
      className={cx(
        "inline-flex shrink-0 items-center justify-center font-medium whitespace-nowrap transition-colors disabled:cursor-not-allowed disabled:opacity-45",
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      {...props}
    >
      {isLoading ? <Spinner size={size === "lg" ? 16 : 14} /> : icon}
      {children}
    </motion.button>
  );
}
