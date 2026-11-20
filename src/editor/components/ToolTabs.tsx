import { useLayoutEffect, useRef, type ReactNode } from "react";
import { cls } from "../../shared/cls";
import { AdjustIcon, CropIcon, LooksIcon, ResizeIcon } from "./icons";

export type Tool = "adjust" | "looks" | "crop" | "resize";

const TOOLS: { id: Tool; label: string; icon: ReactNode }[] = [
  { id: "adjust", label: "Adjust", icon: <AdjustIcon /> },
  { id: "looks", label: "Looks", icon: <LooksIcon /> },
  { id: "crop", label: "Crop", icon: <CropIcon /> },
  { id: "resize", label: "Resize", icon: <ResizeIcon /> },
];

/** The tools, as one segmented row under the photo; a white pill glides to the chosen one */
export default function ToolTabs({ tool, onTool }: { tool: Tool; onTool: (tool: Tool) => void }) {
  const navRef = useRef<HTMLElement>(null);
  const pillRef = useRef<HTMLSpanElement>(null);

  // The pill takes the chosen tab's place (set on the element: it only moves, nothing re-renders)
  useLayoutEffect(() => {
    const active = navRef.current?.querySelector<HTMLElement>(`[data-tool="${tool}"]`);
    const pill = pillRef.current;
    if (!active || !pill) return;
    // Placed at once the first time, glides after that
    const first = pill.dataset.placed !== "1";
    if (first) pill.style.transition = "none";
    pill.style.width = `${active.offsetWidth}px`;
    pill.style.transform = `translateX(${active.offsetLeft}px)`;
    if (first) {
      void pill.offsetWidth; // apply the place before transitions come back
      pill.style.transition = "";
      pill.dataset.placed = "1";
    }
  }, [tool]);

  return (
    <nav className="ed-tabs" aria-label="Tools" ref={navRef}>
      <span className="ed-tab-pill" ref={pillRef} aria-hidden="true" />
      {TOOLS.map((t) => (
        <button
          key={t.id}
          type="button"
          data-tool={t.id}
          className={cls("ed-tab", tool === t.id && "active")}
          aria-pressed={tool === t.id}
          onClick={() => onTool(t.id)}
        >
          {t.icon}
          {t.label}
        </button>
      ))}
    </nav>
  );
}
