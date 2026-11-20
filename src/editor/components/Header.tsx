import { useEffect, useRef, type MouseEvent, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { thumbnailPixels } from "../lib/render";

type Props = {
  name: string;
  /** Under the title: the photo's size, what it will be saved as */
  detail?: ReactNode;
  /** The photo, for the small picture beside its name (once it has opened) */
  pixels?: ImageData;
  /** S / M / L, then the actions */
  children?: ReactNode;
};

/** Pressing the header (not its controls) moves the borderless window */
const dragWindow = (e: MouseEvent) => {
  if (e.button === 0 && !(e.target as HTMLElement).closest("button, input")) getCurrentWindow().startDragging();
};

/** The editor's header, in the Workshop's style: title and photo, then sizes and actions */
export default function Header({ name, detail, pixels, children }: Props) {
  return (
    <header className="ws-header ed-header" onMouseDown={dragWindow}>
      <div className="ws-title-group ed-title-group">
        <div className="ws-title-row">
          <h2 className="ws-title">Photo Editor</h2>
          <span className="ws-badge-pill ed-file" title={name}>
            {pixels && <PhotoThumb pixels={pixels} />}
            <span className="ed-file-name">{name}</span>
          </span>
        </div>
        {detail && <span className="ws-subtitle ed-detail">{detail}</span>}
      </div>
      <div className="ws-header-actions">{children}</div>
    </header>
  );
}

/** Longest side of the picture in the name pill, in pixels (drawn at about three times its size) */
const THUMB_MAX = 48;

function PhotoThumb({ pixels }: { pixels: ImageData }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const thumb = thumbnailPixels(pixels, THUMB_MAX);
    canvas.width = thumb.width;
    canvas.height = thumb.height;
    canvas.getContext("2d")?.putImageData(thumb, 0, 0);
  }, [pixels]);
  return <canvas ref={ref} className="ed-file-thumb" aria-hidden="true" />;
}
