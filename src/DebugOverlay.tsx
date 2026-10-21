import type { Point, RingId, ZoneDetail } from "./dropTarget";

// Drop-position diagnostics. Enabled with `VITE_DEBUG_DROP=1 npm run tauri dev`
// or by adding `?debug` to the page URL. Off by default.
export const DEBUG_DROP =
  import.meta.env.VITE_DEBUG_DROP === "1" || new URLSearchParams(window.location.search).has("debug");

export type DropDebugInfo = {
  type: string;
  raw: Point;
  dpr: number;
  scaleFactor: number;
  point: Point; // where the app thinks the cursor is (CSS pixels)
  center: Point;
  ring: RingId;
  detail: ZoneDetail;
  result: string; // format that will be used on release
};

const fmt = (p: Point) => `${Math.round(p.x)}, ${Math.round(p.y)}`;

export function DebugOverlay({ info }: { info: DropDebugInfo | null }) {
  if (!info) return null;
  const { detail } = info;
  return (
    <>
      <div className="debug-dot" style={{ left: info.point.x, top: info.point.y }} />
      <pre className="debug-readout">
        {`event   ${info.type}
raw     ${fmt(info.raw)}
dpr     ${info.dpr}   scale ${info.scaleFactor}
css     ${fmt(info.point)}
center  ${fmt(info.center)}
delta   ${Math.round(detail.dx)}, ${Math.round(detail.dy)}  r=${Math.round(detail.distance)}
angle   ${detail.angleDeg.toFixed(1)}°  ring ${info.ring}
zone    ${detail.zone.kind === "item" ? `${detail.zone.index} ${detail.zone.item.id}` : detail.zone.kind}
use     ${info.result}`}
      </pre>
    </>
  );
}
