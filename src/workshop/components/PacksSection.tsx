import { useEffect, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { PACK_INFO, percent, sizeLabel, usable, type PackProgress, type PackStatus } from "../../packs/packs";
import type { Packs } from "../../packs/usePacks";
import { cls } from "../../shared/cls";
import { DEVICE_NAME } from "../../shared/platform";

type Props = {
  packs: Packs;
  /** The pack a drop on the ring asked for: shown first, highlighted */
  focus: string | null;
};

/**
 * At the top of the Workshop: the AI features whose models aren't downloaded yet, so a
 * feature missing from someone's ring is found here, with what it takes to get it; then
 * the downloaded ones, to free their space
 */
export default function PacksSection({ packs, focus }: Props) {
  const [confirming, setConfirming] = useState<string | null>(null);
  // Opening the Workshop looks for newer versions (the backend, at most every few hours)
  const { load } = packs;
  useEffect(() => {
    load(true);
  }, [load]);
  if (packs.packs.length === 0) return null;

  const first = (p: PackStatus) => (p.id === focus ? -1 : 0);
  const waiting = packs.packs.filter((p) => !usable(p) || p.state === "update").sort((a, b) => first(a) - first(b));
  const installed = packs.packs.filter((p) => p.state === "installed");
  const missing = waiting.some((p) => p.state !== "update");

  return (
    <div className={cls("ws-packs", missing && "has-missing")}>
      <div className="ws-row-header">
        <span className="ws-section-label">{missing ? "AI features to download" : "AI features"}</span>
        <span className="ws-helper-text">
          {missing ? `They stay on the ring: download one to use it. All run on this ${DEVICE_NAME}.` : "All downloaded"}
        </span>
      </div>

      {waiting.map((pack) => (
        <PackCard
          key={pack.id}
          pack={pack}
          progress={packs.progress[pack.id]}
          error={packs.failed[pack.id]}
          focused={pack.id === focus}
          onInstall={() => packs.install(pack.id)}
          onCancel={() => packs.cancel(pack.id)}
        />
      ))}

      {installed.length > 0 && (
        <div className="ws-packs-installed">
          <span className="ws-helper-text">Downloaded:</span>
          {installed.map((pack) => (
            <span key={pack.id} className="ws-pack-chip" title={PACK_INFO[pack.id]}>
              {pack.name} <span className="ws-pack-chip-size">{sizeLabel(pack.size)}</span>
              {pack.removable &&
                (confirming === pack.id ? (
                  <button
                    type="button"
                    className="ws-pack-remove confirm"
                    onClick={() => {
                      setConfirming(null);
                      packs.remove(pack.id);
                    }}
                    onMouseLeave={() => setConfirming(null)}
                  >
                    Remove?
                  </button>
                ) : (
                  <button
                    type="button"
                    className="ws-pack-remove"
                    title={`Remove ${pack.name} to free ${sizeLabel(pack.size)}; download it again any time`}
                    onClick={() => setConfirming(pack.id)}
                  >
                    ×
                  </button>
                ))}
              {packs.failed[pack.id] && <span className="ws-pack-error">{packs.failed[pack.id]}</span>}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

type CardProps = {
  pack: PackStatus;
  progress: PackProgress | undefined;
  error: string | undefined;
  focused: boolean;
  onInstall: () => void;
  onCancel: () => void;
};

function PackCard({ pack, progress, error, focused, onInstall, onCancel }: CardProps) {
  const size = sizeLabel(pack.download || pack.size);
  const total = progress?.total || pack.total;
  const done = progress?.done ?? pack.done;
  return (
    <div className={cls("ws-pack", focused && "focused", pack.state)}>
      <div className="ws-pack-text">
        <span className="ws-pack-name">
          {pack.name}
          {pack.state === "update" && <span className="ws-pack-tag">Update</span>}
        </span>
        <span className="ws-pack-info">{PACK_INFO[pack.id] ?? pack.targets.join(", ")}</span>
        {error && <span className="ws-pack-error">{error}</span>}
      </div>

      <div className="ws-pack-action">
        {pack.state === "downloading" ? (
          <>
            <div className="ws-pack-progress">
              <div className="ws-pack-bar">
                <div className="ws-pack-fill" style={{ width: `${percent(done, total)}%` }} />
              </div>
              <span className="ws-pack-count">
                {total > 0 ? `${percent(done, total)}% · ${sizeLabel(done)} of ${sizeLabel(total)}` : "Starting…"}
              </span>
            </div>
            <button type="button" className="ws-btn ws-btn-ghost ws-btn-sm" onClick={onCancel} title="Stop; what is downloaded is kept for next time">
              Stop
            </button>
          </>
        ) : pack.state === "restart" ? (
          <button type="button" className="ws-btn ws-btn-primary ws-btn-sm" onClick={() => relaunch().catch(() => {})}>
            Restart to finish
          </button>
        ) : (
          <button type="button" className={cls("ws-btn ws-btn-sm", pack.state === "update" ? "ws-btn-ghost" : "ws-btn-primary")} onClick={onInstall}>
            {error ? "Try again" : pack.state === "update" ? "Update" : "Download"} · {size}
          </button>
        )}
      </div>
    </div>
  );
}
