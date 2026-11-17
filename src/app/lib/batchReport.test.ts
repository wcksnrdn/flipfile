import { describe, expect, it } from "vitest";
import { batchProgressStatus, batchReport, batchStartStatus, failedBatch } from "./batchReport";
import { SAME_FORMAT, STATUS_CLEAR_MS } from "./constants";
import type { BatchResult, Status } from "./types";

const result = (overrides: Partial<BatchResult> = {}): BatchResult => ({
  outputs: ["/out/a.png"],
  errors: [],
  pages: 0,
  bytes_before: 0,
  bytes_after: 0,
  note: null,
  ...overrides,
});

describe("batchStartStatus", () => {
  it("names what is happening to a single file", () => {
    expect(batchStartStatus({ paths: ["a.jpg"], format: "png", preset: null })).toEqual({
      title: "Converting",
      hint: "→ PNG",
      tone: "busy",
      progress: undefined,
    });
    expect(batchStartStatus({ paths: ["a.jpg"], format: "up-2x", preset: null }).title).toBe("Upscaling");
    expect(batchStartStatus({ paths: ["a.mp3"], format: "tr-srt", preset: null }).title).toBe("Listening");
    expect(batchStartStatus({ paths: ["a.jpg"], format: "cmp-light", preset: null }).title).toBe("Compressing");
  });

  it("counts files when there are several", () => {
    const s = batchStartStatus({ paths: ["a", "b", "c"], format: "png", preset: null });
    expect(s.title).toBe("0/3");
    expect(s.progress).toBe(0);
  });

  it("names the preset, or only the preset when the format stays the same", () => {
    expect(batchStartStatus({ paths: ["a"], format: "png", preset: "1080p" }).hint).toBe("→ PNG · 1080p");
    expect(batchStartStatus({ paths: ["a"], format: SAME_FORMAT, preset: "half" }).hint).toBe("→ 50%");
  });
});

describe("batchProgressStatus", () => {
  const busy: Status = { title: "Converting", hint: "→ PNG", tone: "busy" };

  it("shows pages of a single PDF", () => {
    expect(batchProgressStatus(busy, { done: 2, total: 4, unit: "page" }, false)).toMatchObject({
      title: "Page 2/4",
      progress: 0.5,
    });
  });

  it("shows steps of a single upscale as a percentage", () => {
    expect(batchProgressStatus(busy, { done: 1, total: 3, unit: "step" }, false)?.title).toBe("33%");
  });

  it("shows files only when there are several", () => {
    expect(batchProgressStatus(busy, { done: 1, total: 2, unit: "file" }, true)?.title).toBe("1/2");
    expect(batchProgressStatus(busy, { done: 1, total: 1, unit: "file" }, false)).toBe(busy);
  });

  it("leaves a finished status alone", () => {
    const done: Status = { title: "✓ Done", hint: "", tone: "ok" };
    expect(batchProgressStatus(done, { done: 1, total: 2, unit: "page" }, false)).toBe(done);
  });
});

describe("batchReport", () => {
  const job = (paths: string[], format: string, preset: string | null = null) => ({ paths, format, preset });

  it("reports a plain conversion", () => {
    const r = batchReport(job(["a.jpg"], "png"), result());
    expect(r.status).toEqual({ title: "✓ Done", hint: "1 file → PNG · click to show", tone: "ok", outputs: ["/out/a.png"] });
    expect(r.clearAfterMs).toBe(STATUS_CLEAR_MS.result);
    expect(r.toast).toBeNull();
  });

  it("adds the backend's note", () => {
    expect(batchReport(job(["a.docx"], "pdf"), result({ note: "Word" })).status.hint).toBe("1 file → PDF · Word · click to show");
  });

  it("reports images merged into one PDF, GIF or presentation", () => {
    const merged = result({ outputs: ["/out/all.pdf"] });
    expect(batchReport(job(["a.jpg", "b.jpg", "c.jpg"], "pdf"), merged).status.hint).toMatch(/^3 images → 1 PDF/);
    expect(batchReport(job(["a.jpg", "b.jpg"], "gif-anim"), merged).status.hint).toMatch(/^2 images → GIF/);
    expect(batchReport(job(["a.jpg", "b.jpg"], "pptx"), merged).status.hint).toMatch(/^2 images → 1 presentation/);
  });

  it("does not call office documents merged slides", () => {
    const r = batchReport(job(["a.docx", "b.docx"], "pptx"), result({ outputs: ["/out/a.pptx"] }));
    expect(r.status.hint).toMatch(/^1 file → Slides/);
  });

  it("counts PDF pages rendered to images", () => {
    expect(batchReport(job(["a.pdf"], "png"), result({ pages: 5 })).status.hint).toMatch(/^5 pages → PNG/);
  });

  it("reports what a compress level saved, with a bubble", () => {
    const r = batchReport(
      job(["a.png"], "cmp-light"),
      result({ bytes_before: 1_000_000, bytes_after: 400_000, note: "PNG → JPG · Fine detail softens" })
    );
    expect(r.status.hint).toBe("1.0 MB → 400 KB · −60% · PNG → JPG · click to show");
    expect(r.toast).toMatchObject({ tone: "info", title: "Compressed · Light", stat: "−60%" });
  });

  it("says when a file was already compact", () => {
    const r = batchReport(job(["a.png"], "cmp-strong"), result({ bytes_before: 1_000_000, bytes_after: 1_000_000 }));
    expect(r.status.hint).toMatch(/^Already compact · 1.0 MB/);
  });

  it("reports a failure, with a bubble and a shorter linger", () => {
    const r = batchReport(job(["a.jpg"], "png"), result({ outputs: [], errors: ["\"a.jpg\": not an image"] }));
    expect(r.status).toMatchObject({ title: "Failed", hint: "\"a.jpg\": not an image", tone: "err" });
    expect(r.clearAfterMs).toBe(STATUS_CLEAR_MS.failure);
    expect(r.toast).toMatchObject({ tone: "error", title: "Couldn't convert" });
  });

  it("reports a partial failure", () => {
    const r = batchReport(job(["a.jpg", "b.jpg"], "png"), result({ errors: ["broken"] }));
    expect(r.status.title).toBe("1 done, 1 failed");
    expect(r.clearAfterMs).toBe(STATUS_CLEAR_MS.result);
  });
});

describe("failedBatch", () => {
  it("turns a thrown command into a batch with one error", () => {
    expect(failedBatch(new Error("boom"))).toMatchObject({ outputs: [], errors: ["Error: boom"] });
  });
});
