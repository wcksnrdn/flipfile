import { useState } from "react";
import { cls } from "../shared/cls";
import { useTransient } from "../shared/useTransient";
import DocBody, { EmptyPanel } from "./components/DocBody";
import { PearlGradient, Sparkle } from "./components/icons";
import { AskBox, DocTabs, FileCard, PanelFooter, PanelHeader, SavedBar } from "./components/PanelChrome";
import { useDocumentExport } from "./hooks/useDocumentExport";
import { useClock, useFollowAnswers, usePanelDrop, usePanelGlass, usePanelShortcuts } from "./hooks/usePanelWindow";
import { useSummaryJobs } from "./hooks/useSummaryJobs";
import "./styles/index.css";

// How long an error notice stays at the bottom of the panel
const NOTICE_MS = 4000;

/** The AI Summary panel, in its own window beside the circle (src-tauri/src/summary/window.rs) */
export default function SummaryPanel() {
  const [notice, showNotice] = useTransient<string>(NOTICE_MS);
  const jobs = useSummaryJobs(showNotice);
  const { state, doc, busy, finished } = jobs;
  const native = usePanelGlass();
  const dropping = usePanelDrop(showNotice);
  const now = useClock(busy);
  const bodyRef = useFollowAnswers(doc);
  const [draft, setDraft] = useState("");
  const { flash, copy, save } = useDocumentExport({
    file: state.active,
    finished,
    onSaved: jobs.markSaved,
    onError: showNotice,
  });
  usePanelShortcuts({ draft, clearDraft: () => setDraft(""), close: jobs.close, copy, save });

  return (
    <div className={cls("sp", native && "native", busy && "busy", dropping && "dropping")}>
      <PearlGradient />
      <div className="sp-glow edge" aria-hidden="true" />
      <div className="sp-glow haze" aria-hidden="true" />

      <PanelHeader busy={busy} finished={finished} flash={flash} onCopy={copy} onSave={save} onClose={jobs.close} />
      <DocTabs docs={state.docs} active={state.active} onPick={jobs.pick} />

      {doc ? (
        <>
          <FileCard doc={doc} />
          <div className="sp-body" key={state.active} ref={bodyRef}>
            <DocBody
              doc={doc}
              checked={jobs.checked}
              onCheck={jobs.toggleCheck}
              onAsk={jobs.canAsk ? jobs.ask : null}
              onRetry={() => jobs.redo(null)}
            />
          </div>
          {finished && (
            <AskBox draft={draft} onDraft={setDraft} canAsk={jobs.canAsk} asking={jobs.asking} onAsk={jobs.ask} />
          )}
          {jobs.savedPath && <SavedBar path={jobs.savedPath} />}
          <PanelFooter doc={doc} busy={busy} now={now} onRedo={jobs.redo} />
        </>
      ) : (
        <EmptyPanel />
      )}

      {notice && <div className="sp-notice">{notice}</div>}
      {dropping && (
        <div className="sp-drop">
          <Sparkle className="live" />
          <span>Drop to summarize</span>
        </div>
      )}
    </div>
  );
}
