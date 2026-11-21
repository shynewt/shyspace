import { ArrowRight } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "../../ui";

export function SubmitButton({
  isBusy,
  isDisabled,
  icon,
  children,
}: {
  isBusy: boolean;
  isDisabled: boolean;
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <Button
      type="submit"
      variant="primary"
      size="lg"
      isLoading={isBusy}
      disabled={isDisabled}
      icon={icon}
      className="group mt-2"
    >
      {children}
      {!isBusy && <ArrowRight size={15} className="transition-transform group-hover:translate-x-0.5" />}
    </Button>
  );
}
