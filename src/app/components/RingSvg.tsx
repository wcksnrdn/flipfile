import type { CSSProperties } from "react";
import {
  HOVER_POP,
  LABEL_RADIUS,
  RING_OUTER,
  STAGE_SIZE,
  angleOf,
  type Item,
  type Ring,
  type RingId,
} from "../../rings/dropTarget";
import { AI_SUMMARY } from "../../workshop/workshop";

type Props = {
  rings: Ring[];
  paths: Record<RingId, string[]>;
  current: RingId;
  hovered: string | null;
  preset: string | null;
  /** The small line under a slice's label (a compress level shows its estimate there) */
  subOf: (item: Item) => string | undefined;
};

/** Slice-shaped format segments, one layer per ring. Selection is by direction (see zoneAt) */
export default function RingSvg({ rings, paths, current, hovered, preset, subOf }: Props) {
  return (
    <svg className="ring" viewBox={`${-STAGE_SIZE / 2} ${-STAGE_SIZE / 2} ${STAGE_SIZE} ${STAGE_SIZE}`} aria-hidden="true">
      <RingGradients />
      {rings.map((r) => (
        <g key={r.id} className={`ring-layer ${r.id === current ? "current" : ""}`}>
          {r.items.map((item, i) => (
            <Segment
              key={`${item.id}-${i}`}
              ring={r}
              item={item}
              index={i}
              path={paths[r.id][i]}
              active={hovered === item.id || (!!item.preset && preset === item.id)}
              sub={subOf(item)}
            />
          ))}
        </g>
      ))}
    </svg>
  );
}

type SegmentProps = {
  ring: Ring;
  item: Item;
  index: number;
  path: string;
  active: boolean;
  sub: string | undefined;
};

function Segment({ ring, item, index, path, active, sub }: SegmentProps) {
  const angle = angleOf(ring, index);
  const lx = Math.cos(angle) * LABEL_RADIUS;
  const ly = Math.sin(angle) * LABEL_RADIUS;
  const style = {
    "--i": index,
    "--dx": `${Math.cos(angle) * HOVER_POP}px`,
    "--dy": `${Math.sin(angle) * HOVER_POP}px`,
  } as CSSProperties;
  const cls = [
    "segment",
    active && "active",
    item.disabled && "disabled",
    item.nav && "nav",
    ring.id === "category" && "category-item",
    item.id === AI_SUMMARY && "ai",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <g className={cls} style={style}>
      <g className="segment-inner">
        <path className="segment-body" d={path} />
        <path className="segment-rim" d={path} />
        <text className="segment-label" x={lx} y={item.sub ? ly - 4 : ly}>
          {item.label}
        </text>
        {item.sub && (
          <text className="segment-sub" x={lx} y={ly + 8}>
            {sub}
          </text>
        )}
      </g>
    </g>
  );
}

function RingGradients() {
  return (
    <defs>
      <linearGradient
        id="seg-rim"
        gradientUnits="userSpaceOnUse"
        x1={-RING_OUTER}
        y1={-RING_OUTER}
        x2={RING_OUTER}
        y2={RING_OUTER}
      >
        <stop offset="0" stopColor="#fff" stopOpacity="0.7" />
        <stop offset="0.4" stopColor="#fff" stopOpacity="0.12" />
        <stop offset="0.65" stopColor="#fff" stopOpacity="0.05" />
        <stop offset="1" stopColor="#fff" stopOpacity="0.35" />
      </linearGradient>
      <linearGradient id="seg-active" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stopColor="var(--accent)" stopOpacity="0.95" />
        <stop offset="1" stopColor="var(--accent-2)" stopOpacity="0.85" />
      </linearGradient>
      {/* Pearl for the AI Summary slice: light, not colour */}
      <linearGradient id="seg-ai" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stopColor="#ffffff" />
        <stop offset="0.55" stopColor="#d6dfeb" />
        <stop offset="1" stopColor="#ffffff" />
      </linearGradient>
    </defs>
  );
}
