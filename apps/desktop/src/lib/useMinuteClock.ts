import { useEffect, useState } from "react";

/** The time now, updated as each minute starts, for clocks that show hours and minutes. */
export function useMinuteClock(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    const waitForNextMinute = () => {
      timer = setTimeout(
        () => {
          setNow(new Date());
          waitForNextMinute();
        },
        60_000 - (Date.now() % 60_000),
      );
    };
    waitForNextMinute();
    return () => clearTimeout(timer);
  }, []);
  return now;
}
