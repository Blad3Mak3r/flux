import type { HTMLAttributes } from "react";

/**
 * Text cut with an ellipsis when it does not fit; while the pointer is over it, it slides from side
 * to side to show the rest. The box must have a bounded width; other props go to it.
 */
export function Marquee({ className, children, ...rest }: HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      {...rest}
      className={className ? `marquee ${className}` : "marquee"}
      onPointerEnter={(event) => {
        const box = event.currentTarget;
        const hidden = box.scrollWidth - box.clientWidth;
        if (hidden > 0) {
          box.style.setProperty("--marquee-shift", `${-hidden}px`);
          // About 40 px a second, so a long name reads at the same pace as a short one.
          box.style.setProperty("--marquee-time", `${Math.max(2, hidden / 40)}s`);
          box.dataset.scrolling = "true";
        }
        rest.onPointerEnter?.(event);
      }}
      onPointerLeave={(event) => {
        delete event.currentTarget.dataset.scrolling;
        rest.onPointerLeave?.(event);
      }}
    >
      <span className="marquee-text">{children}</span>
    </span>
  );
}
