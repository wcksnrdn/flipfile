import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { CORE_OPEN_SCALE, CORE_SIZE, STAGE_SIZE } from "../rings/dropTarget";
import { DEBUG_DROP, DebugOverlay } from "../debug/DebugOverlay";
import EditorPanel from "../editor/EditorPanel";
import { photoName } from "../editor/hooks/usePhoto";
import UpdateDialog from "../update/UpdateDialog";
import WorkshopPanel from "../workshop/WorkshopPanel";
import { PHOTO_EDITOR } from "../workshop/workshop";
import ChipDragGhost from "./components/ChipDragGhost";
import CoreDisc from "./components/CoreDisc";
import RingSvg from "./components/RingSvg";
import { useAltPresets } from "./hooks/useAltPresets";
import { useAiSupport, useSettingsSummary } from "./hooks/useBackendSettings";
import { useBackgroundStatus } from "./hooks/useBackgroundStatus";
import { useChipDrag } from "./hooks/useChipDrag";
import { useClipboardPaste } from "./hooks/useClipboardPaste";
import { useCompressEstimates } from "./hooks/useCompressEstimates";
import { useConversion } from "./hooks/useConversion";
import { useCoreAnchor } from "./hooks/useCoreAnchor";
import { useCoreGestures } from "./hooks/useCoreGestures";
import { useEditor } from "./hooks/useEditor";
import { useFileDrop } from "./hooks/useFileDrop";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useNativeGlass } from "./hooks/useNativeGlass";
import { useRingNavigation } from "./hooks/useRingNavigation";
import { useRingPointer } from "./hooks/useRingPointer";
import { useRingSelection } from "./hooks/useRingSelection";
import { useScaleFactor } from "./hooks/useScaleFactor";
import { useStatus } from "./hooks/useStatus";
import { useUpdater } from "./hooks/useUpdater";
import { useWorkshop } from "./hooks/useWorkshop";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { STATUS_CLEAR_MS } from "./lib/constants";
import { coreText } from "./lib/coreText";
import { GLASS_OPEN, sendGlass } from "./lib/glass";
import type { Execute } from "./lib/types";
import "./styles/index.css";

const STAGE_STYLE = {
  "--stage": `${STAGE_SIZE}px`,
  "--core": `${CORE_SIZE}px`,
  "--core-open-scale": CORE_OPEN_SCALE,
} as CSSProperties;

export default function App() {
  // Backend state and services
  const { status, ...statusActions } = useStatus();
  const updater = useUpdater();
  const aiSupported = useAiSupport();
  const settingsSummary = useSettingsSummary();
  const scale = useScaleFactor();
  useBackgroundStatus(statusActions);

  // The rings and what is in hand
  const { coreRef, coreCenter } = useCoreAnchor();
  const workspaces = useWorkspaces();
  const nav = useRingNavigation(workspaces.active, aiSupported);
  const selection = useRingSelection({ nav, coreCenter, scale });
  const workshop = useWorkshop({ workspaces, nav, coreCenter, clearHover: () => selection.setHovered(null) });
  const editor = useEditor({ coreCenter });
  const { chipDrag, startChipDrag } = useChipDrag({
    workspace: workspaces.active,
    ringToEdit: workshop.ringToEdit,
    nav,
    coreCenter,
    setHovered: selection.setHovered,
    setSlots: workspaces.setSlots,
  });

  // The ring is open while dragging files in, choosing a format for a paste, or editing in the Workshop.
  const open = selection.dragging || selection.picking !== null || workshop.open;
  const aiming = open && !workshop.open;

  const nativeGlass = useNativeGlass({ coreCenter, open, panelOpen: workshop.open || editor.open });
  const { estimates, sliceSub } = useCompressEstimates({
    enabled: aiming,
    ring: nav.built.rings[nav.ringId],
    getPaths: selection.getPaths,
  });

  // Input: drag & drop, paste-and-click, Option/Alt, keyboard, the core itself
  const convert = useConversion(statusActions);
  // The Edit slice opens the photo in the Photo Editor; every other one converts
  const execute: Execute = (paths, format, toDownloads, preset) => {
    if (format !== PHOTO_EDITOR) return convert(paths, format, toDownloads, preset);
    if (workshop.open) workshop.toggle(false, true);
    editor.openPhoto(paths[0], toDownloads);
  };
  const paste = useClipboardPaste(selection, statusActions);
  // While the Photo Editor is open, a photo dropped on the window opens in it instead
  useFileDrop({ selection, scale, execute, enabled: !editor.open });
  useAltPresets({ enabled: aiming, nav, selection });
  useKeyboardShortcuts({
    onQuit: () => invoke("quit_app"),
    // The Photo Editor has its own shortcuts (src/editor/hooks/useEditorShortcuts.ts)
    onToggleWorkshop: () => {
      if (!editor.open) workshop.toggle();
    },
    onPaste: () => {
      if (!editor.open && !selection.dragging && status?.tone !== "busy") paste();
    },
    onEscape: () => {
      if (editor.open) return;
      if (workshop.open) workshop.toggle(false);
      else selection.closeRing();
    },
  });
  const pointer = useRingPointer({
    coreRef,
    coreCenter,
    nav,
    selection,
    workshop,
    workspace: workspaces.active,
    chipDragging: chipDrag !== null,
    execute,
  });
  const coreGestures = useCoreGestures({
    onClick: () => {
      const first = status?.outputs?.[0];
      // Clicking the center cancels a paste, like dropping on it
      if (selection.picking) selection.closeRing();
      else if (status?.ai) invoke("summary_show").catch(() => {});
      else if (first) revealItemInDir(first).catch(() => {});
    },
    onContextMenu: () => {
      if (!editor.open) workshop.toggle();
    },
  });

  const text = coreText({
    open,
    dragging: selection.dragging,
    picking: selection.picking,
    workshopOpen: workshop.open,
    chipDrag,
    ringId: nav.ringId,
    rings: nav.built.rings,
    hovered: selection.hovered,
    preset: selection.preset,
    estimates,
    fileCount: selection.fileCount,
    mergeable: selection.mergeable,
    settingsSummary,
    activeWorkspace: workspaces.active,
    status,
    editing: editor.request && photoName(editor.request.path),
  });
  const busy = !open && status?.tone === "busy";
  const aiBusy = busy && !!status?.ai;

  return (
    <main
      className={[
        selection.picking && selection.hovered && "pick-ready",
        // (kept until the panel has left and the window has shrunk)
        (workshop.open || workshop.panel !== "hidden") && "workshop-open",
        editor.open && "editor-open",
      ]
        .filter(Boolean)
        .join(" ")}
      onMouseMove={pointer.onMouseMove}
      onClick={pointer.onClick}
    >
      <div className={`stage ${open ? "open" : ""} ${nativeGlass ? "native" : ""}`} style={STAGE_STYLE}>
        <RingSvg
          rings={nav.ringList}
          paths={nav.built.paths}
          current={nav.ringId}
          hovered={selection.hovered}
          preset={selection.preset}
          subOf={sliceSub}
        />
        <CoreDisc
          coreRef={coreRef}
          text={text}
          open={open}
          tone={!open && status ? status.tone : ""}
          busy={busy}
          aiBusy={aiBusy}
          progress={busy ? status?.progress : undefined}
          clickable={!open && (!!status?.outputs?.length || !!status?.ai)}
          chipOnCore={chipDrag?.targetSlice === "core"}
          gestures={coreGestures}
          onOpenWorkshop={() => workshop.toggle(true)}
        />
      </div>

      {workshop.panel !== "hidden" && (
        <WorkshopPanel
          leaving={workshop.panel === "leaving"}
          state={workspaces.state}
          activeWorkspace={workspaces.active}
          editingRing={workshop.editingRing}
          {...workshop.panelHandlers}
          onStartChipDrag={startChipDrag}
          onWindowResized={() => sendGlass(coreCenter(), GLASS_OPEN).catch(() => {})}
          onCheckUpdate={updater.checkNow}
          onClose={() => workshop.toggle(false)}
        />
      )}

      {editor.request && editor.panel !== "hidden" && (
        <EditorPanel
          request={editor.request}
          leaving={editor.panel === "leaving"}
          onOpen={(path) => editor.openPhoto(path, false)}
          onWindowResized={editor.onWindowResized}
          // The circle beside the editor says so; clicking it shows the file
          onSaved={(output) =>
            statusActions.show({ title: "Saved", hint: photoName(output), tone: "ok", outputs: [output] }, STATUS_CLEAR_MS.result)
          }
          onClose={editor.close}
        />
      )}

      {chipDrag && <ChipDragGhost drag={chipDrag} />}

      <UpdateDialog {...updater.dialogProps} />

      {DEBUG_DROP && <DebugOverlay info={selection.debugInfo} />}
    </main>
  );
}
