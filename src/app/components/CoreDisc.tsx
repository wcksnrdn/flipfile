import type { MouseEvent, RefObject } from "react";
import { PROGRESS_LEN, PROGRESS_R } from "../lib/constants";
import type { CoreText } from "../lib/coreText";

type Props = {
  coreRef: RefObject<HTMLDivElement | null>;
  text: CoreText;
  /** The ring is showing: the badge is a label, not the Workshop button */
  open: boolean;
  tone: string;
  busy: boolean;
  /** An AI summary is being written: the Apple Intelligence halo instead of the progress ring */
  aiBusy: boolean;
  /** 0..1 when known, else a spinning arc */
  progress: number | undefined;
  clickable: boolean;
  /** A Workshop chip is aimed at the core: it will be added to the ring */
  chipOnCore: boolean;
  gestures: {
    onMouseDown: (e: MouseEvent) => void;
    onMouseMove: (e: MouseEvent) => void;
    onMouseUp: () => void;
    onMouseLeave: () => void;
    onContextMenu: (e: MouseEvent) => void;
  };
  onOpenWorkshop: () => void;
};

const titleSize = (title: string) => (title.length > 10 ? "long" : title.length > 7 ? "medium" : "");

/** The centre of the ring: drag it to move the window, click it to reveal the result, or right-click for Ring Workshop */
export default function CoreDisc({
  coreRef,
  text: { title, hint, badge },
  open,
  tone,
  busy,
  aiBusy,
  progress,
  clickable,
  chipOnCore,
  gestures,
  onOpenWorkshop,
}: Props) {
  const cls = ["core", "glass", tone, clickable && "clickable", aiBusy && "ai", chipOnCore && "drop-core"]
    .filter(Boolean)
    .join(" ");

  return (
    <div ref={coreRef} className={cls} {...gestures}>
      {/* Apple Intelligence at work: a turning ring of its colours */}
      {aiBusy && (
        <>
          <span className="ai-halo-glow" aria-hidden="true" />
          <span className="ai-halo" aria-hidden="true" />
        </>
      )}
      {busy && !aiBusy && <ProgressRing progress={progress} />}
      <span className={`core-title ${titleSize(title)}`}>{title}</span>
      <span className="core-hint">{hint}</span>
      {badge &&
        (open ? (
          <span className="core-badge">{badge}</span>
        ) : (
          <button
            type="button"
            className="core-badge core-workshop-btn"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              onOpenWorkshop();
            }}
            title="Open Ring Workshop (Cmd+,)"
          >
            {badge}
          </button>
        ))}
    </div>
  );
}

// Determinate ring when progress is known, otherwise a spinning arc
function ProgressRing({ progress }: { progress: number | undefined }) {
  const indeterminate = progress === undefined;
  return (
    <svg
      className={`progress ${indeterminate ? "indeterminate" : ""}`}
      viewBox={`${-PROGRESS_R - 2} ${-PROGRESS_R - 2} ${PROGRESS_R * 2 + 4} ${PROGRESS_R * 2 + 4}`}
    >
      <circle className="progress-track" r={PROGRESS_R} />
      <circle
        className="progress-bar"
        r={PROGRESS_R}
        strokeDasharray={indeterminate ? `${PROGRESS_LEN * 0.22} ${PROGRESS_LEN}` : PROGRESS_LEN}
        strokeDashoffset={indeterminate ? 0 : PROGRESS_LEN * (1 - progress)}
      />
    </svg>
  );
}
