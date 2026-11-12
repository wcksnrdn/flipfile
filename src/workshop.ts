import {
  RINGS as BASE_RINGS,
  alignedSubRing,
  buildRing,
  segmentPath,
  type Item,
  type Ring,
  type RingId,
} from "./dropTarget";

const IS_MAC = /Mac/i.test(navigator.userAgent);

/** AI summary of a document by Apple Intelligence (src/SummaryPanel.tsx); macOS only */
export const AI_SUMMARY = "ai-summary";

// Whether this computer can write AI summaries: assumed on macOS until the backend says.
// Without it the lobby keeps its four rooms and other rings show Summary greyed out.
let aiSupport = IS_MAC;
export function setAiSupport(supported: boolean) {
  aiSupport = supported;
}
export const hasAiSupport = () => aiSupport;

export type CatalogCategory = "photo" | "photo-tools" | "office" | "data" | "video" | "music" | "speech" | "nav";

export type CatalogEntry = Item & {
  title: string;
  category: CatalogCategory;
};

export const CATALOG_CATEGORIES: { id: "all" | CatalogCategory; label: string }[] = [
  { id: "all", label: "All" },
  { id: "photo", label: "Photo" },
  { id: "photo-tools", label: "Photo Tools" },
  { id: "office", label: "Office & Docs" },
  { id: "data", label: "Data & Sheets" },
  { id: "video", label: "Video" },
  { id: "music", label: "Music" },
  { id: "speech", label: "Transcribe" },
  { id: "nav", label: "Sub-Rings" },
];

export const EDITABLE_RINGS: { id: RingId; label: string; group: string }[] = [
  { id: "category", label: "Lobby (Rooms)", group: "Main" },
  { id: "photo", label: "Photo Main", group: "Photo" },
  { id: "photo-formats", label: "Photo Formats", group: "Photo" },
  { id: "photo-pro", label: "Photo Pro / 3D", group: "Photo" },
  { id: "photo-tools", label: "Photo Tools", group: "Photo" },
  { id: "office", label: "Office Main", group: "Office" },
  { id: "office-docs", label: "Office Docs", group: "Office" },
  { id: "office-data", label: "Office Data", group: "Office" },
  { id: "video", label: "Video Main", group: "Video" },
  { id: "video-tools", label: "Video Tools", group: "Video" },
  { id: "music", label: "Music Main", group: "Music" },
  { id: "music-more", label: "Music More", group: "Music" },
  { id: "music-transcribe", label: "Music Transcribe", group: "Music" },
  { id: "video-transcribe", label: "Video Subtitles", group: "Video" },
];

export const CATALOG: CatalogEntry[] = [
  // ─── Photo Formats ────────────────────────────────────────────────────────
  { id: "jpg", label: "JPG", title: "JPG Image", category: "photo" },
  { id: "png", label: "PNG", title: "PNG Lossless", category: "photo" },
  { id: "webp", label: "WEBP", title: "WebP Image", category: "photo" },
  { id: "heic", label: "HEIC", title: "Apple HEIC", category: "photo", disabled: !IS_MAC },
  { id: "avif", label: "AVIF", title: "AVIF Image", category: "photo" },
  { id: "gif", label: "GIF", title: "Static GIF", category: "photo" },
  { id: "tiff", label: "TIFF", title: "TIFF Archival", category: "photo" },
  { id: "bmp", label: "BMP", title: "Bitmap BMP", category: "photo" },
  { id: "ico", label: "ICO", sub: "multi", title: "Windows Multi-Size .ICO", category: "photo" },
  { id: "icns", label: "ICNS", sub: "macOS", title: "macOS App Icon .ICNS", category: "photo" },
  { id: "svg", label: "SVG", sub: "embed", title: "SVG Vector Wrapper", category: "photo" },
  { id: "tga", label: "TGA", sub: "game", title: "Truevision TGA (Game/VFX)", category: "photo" },
  { id: "exr", label: "EXR", sub: "32f HDR", title: "OpenEXR 32-bit HDR", category: "photo" },
  { id: "hdr", label: "HDR", sub: "RGBE", title: "Radiance HDR", category: "photo" },
  { id: "qoi", label: "QOI", sub: "fast", title: "Quite OK Image (QOI)", category: "photo" },
  { id: "ppm", label: "PPM", sub: "netpbm", title: "Netpbm PPM", category: "photo" },
  { id: "jpeg", label: "JPEG", title: "JPEG (.jpeg ext)", category: "photo" },

  // ─── Photo Smart Tools ────────────────────────────────────────────────────
  { id: "nobg", label: "Remove", sub: "BG", title: "AI Remove Background", category: "photo-tools", disabled: !IS_MAC },
  { id: "up-2x", label: "Upscale", sub: "2×", title: "AI Upscale 2×", category: "photo-tools" },
  { id: "up-4x", label: "Upscale", sub: "4×", title: "AI Upscale 4×", category: "photo-tools" },
  { id: "clean", label: "Clean", sub: "EXIF", title: "Strip EXIF & GPS Metadata", category: "photo-tools" },
  { id: "square", label: "Square", sub: "1:1", title: "Pad to 1:1 Square", category: "photo-tools" },
  { id: "gif-anim", label: "GIF", sub: "anim", title: "Combine Photos → Animated GIF", category: "photo-tools" },
  { id: "cmp-auto", label: "Auto", sub: "smaller", title: "Auto Compress Photo", category: "photo-tools" },
  { id: "cmp-1m", label: "1 MB", sub: "max", title: "Compress Under 1 MB", category: "photo-tools" },
  { id: "cmp-500k", label: "500 KB", sub: "max", title: "Compress Under 500 KB", category: "photo-tools" },
  { id: "cmp-200k", label: "200 KB", sub: "max", title: "Compress Under 200 KB", category: "photo-tools" },
  { id: "cmp-100k", label: "100 KB", sub: "max", title: "Compress Under 100 KB", category: "photo-tools" },
  { id: "cmp-lossless", label: "Lossless", sub: "exact", title: "Lossless Optimize", category: "photo-tools" },
  { id: "rot-r", label: "Rotate", sub: "90° ›", title: "Rotate 90° Clockwise", category: "photo-tools" },
  { id: "rot-l", label: "Rotate", sub: "‹ 90°", title: "Rotate 90° Counter-Clockwise", category: "photo-tools" },
  { id: "rot-180", label: "Rotate", sub: "180°", title: "Rotate 180°", category: "photo-tools" },
  { id: "flip-h", label: "Flip", sub: "horiz", title: "Flip Horizontal", category: "photo-tools" },
  { id: "flip-v", label: "Flip", sub: "vert", title: "Flip Vertical", category: "photo-tools" },
  { id: "grayscale", label: "B&W", sub: "gray", title: "Convert to Grayscale (B&W)", category: "photo-tools" },

  // ─── Office & Documents ───────────────────────────────────────────────────
  { id: "pdf", label: "PDF", title: "PDF Document (or Merge Photos)", category: "office" },
  { id: "docx", label: "WORD", sub: "docx", title: "Word Document (.docx)", category: "office" },
  { id: "docx-ocr", label: "WORD", sub: "OCR", title: "Word OCR (Read Scanned Text)", category: "office" },
  { id: "pptx", label: "SLIDES", sub: "pptx", title: "PowerPoint Presentation (.pptx)", category: "office" },
  { id: "md", label: "MD", sub: "markdown", title: "Markdown (.md)", category: "office" },
  { id: "txt", label: "TXT", sub: "plain", title: "Plain Text (.txt)", category: "office" },
  { id: "html", label: "HTML", sub: "web", title: "HTML Web Page (.html)", category: "office" },
  { id: "rtf", label: "RTF", sub: "rich text", title: "Rich Text Format (.rtf)", category: "office" },
  { id: "epub", label: "EPUB", sub: "e-book", title: "EPUB E-Book (.epub)", category: "office" },
  { id: "odt", label: "ODT", sub: "open doc", title: "OpenDocument Text (.odt)", category: "office" },
  { id: AI_SUMMARY, label: "AI ✦", sub: "summary", title: "AI Summary (Apple Intelligence, on this Mac)", category: "office", disabled: !IS_MAC },

  // ─── Spreadsheets & Developer Data ────────────────────────────────────────
  { id: "xlsx", label: "EXCEL", sub: "xlsx", title: "Excel Workbook (.xlsx)", category: "data" },
  { id: "csv", label: "CSV", title: "CSV Comma-Separated (.csv)", category: "data" },
  { id: "tsv", label: "TSV", sub: "tabs", title: "TSV Tab-Separated (.tsv)", category: "data" },
  { id: "json", label: "JSON", title: "JSON Records (.json)", category: "data" },
  { id: "yaml", label: "YAML", title: "YAML Data (.yaml)", category: "data" },
  { id: "xml", label: "XML", title: "XML Workbook (.xml)", category: "data" },
  { id: "sql", label: "SQL", sub: "insert", title: "SQL CREATE + INSERT (.sql)", category: "data" },
  { id: "md-table", label: "MD", sub: "table", title: "Markdown Table (.md)", category: "data" },

  // ─── Video Formats & Tools ────────────────────────────────────────────────
  { id: "mp4", label: "MP4", title: "MP4 Video (H.264)", category: "video" },
  { id: "mov", label: "MOV", title: "QuickTime MOV Video", category: "video" },
  { id: "webm", label: "WEBM", title: "WebM Video (VP9)", category: "video" },
  { id: "mkv", label: "MKV", title: "Matroska MKV Video", category: "video" },
  { id: "avi", label: "AVI", title: "AVI Video", category: "video" },
  { id: "flv", label: "FLV", title: "Flash FLV Video", category: "video" },
  { id: "vid-gif", label: "GIF", sub: "clip", title: "Video Clip → Animated GIF", category: "video" },
  { id: "vid-compress", label: "Compress", sub: "smaller", title: "Compress Video Size", category: "video" },
  { id: "vid-mute", label: "Mute", sub: "no audio", title: "Remove Audio from Video", category: "video" },
  { id: "vid-mp3", label: "Audio", sub: "extract", title: "Extract Audio from Video", category: "video" },
  { id: "vid-1080p", label: "1080p", sub: "Full HD", title: "Resize Video to 1080p", category: "video" },
  { id: "vid-720p", label: "720p", sub: "HD", title: "Resize Video to 720p", category: "video" },

  // ─── Music / Audio Formats ────────────────────────────────────────────────
  { id: "mp3", label: "MP3", title: "MP3 Audio", category: "music" },
  { id: "wav", label: "WAV", title: "WAV Uncompressed PCM", category: "music" },
  { id: "m4a", label: "M4A", sub: "AAC", title: "M4A / AAC Audio", category: "music" },
  { id: "flac", label: "FLAC", title: "FLAC Lossless Audio", category: "music" },
  { id: "ogg", label: "OGG", title: "OGG Vorbis Audio", category: "music" },
  { id: "aiff", label: "AIFF", sub: "studio", title: "AIFF Studio Audio", category: "music" },
  { id: "opus", label: "OPUS", title: "Opus Audio", category: "music" },
  { id: "aac", label: "AAC", title: "Raw AAC Audio", category: "music" },
  { id: "caf", label: "CAF", sub: "Apple", title: "Apple CoreAudio (.caf)", category: "music" },
  { id: "alac", label: "ALAC", sub: "lossless", title: "Apple Lossless (.m4a)", category: "music" },
  { id: "wma", label: "WMA", title: "Windows Media Audio (.wma)", category: "music" },

  // ─── Transcribe (speech to text, on this computer) ───────────────────────
  { id: "tr-srt", label: "SRT", sub: "subtitles", title: "Transcribe → Subtitles (.srt)", category: "speech" },
  { id: "tr-vtt", label: "VTT", sub: "web subs", title: "Transcribe → Web Subtitles (.vtt)", category: "speech" },
  { id: "tr-txt", label: "TXT", sub: "transcript", title: "Transcribe → Plain Text (.txt)", category: "speech" },
  { id: "tr-docx", label: "WORD", sub: "transcript", title: "Transcribe → Word Document (.docx)", category: "speech" },
  { id: "tr-md", label: "MD", sub: "notes", title: "Transcribe → Markdown Notes (.md)", category: "speech" },

  // ─── Room & Sub-Ring Navigation Shortcuts ─────────────────────────────────
  { id: "cat-photo", label: "Photo", nav: "photo", title: "Open Photo Room", category: "nav" },
  { id: "cat-office", label: "Office", nav: "office", title: "Open Office Room", category: "nav" },
  { id: "cat-video", label: "Video", nav: "video", title: "Open Video Room", category: "nav" },
  { id: "cat-music", label: "Music", nav: "music", title: "Open Music Room", category: "nav" },
  { id: "photo-formats-nav", label: "Formats", sub: "•••", nav: "photo-formats", title: "Open Photo Formats Ring", category: "nav" },
  { id: "photo-pro-nav", label: "Pro / 3D", sub: "•••", nav: "photo-pro", title: "Open Photo Pro / 3D Ring", category: "nav" },
  { id: "photo-tools-nav", label: "Tools", sub: "✦", nav: "photo-tools", title: "Open Photo Tools Ring", category: "nav" },
  { id: "photo-compress-nav", label: "Compress", sub: "size", nav: "photo-compress", title: "Open Photo Compress Ring", category: "nav" },
  { id: "photo-transform-nav", label: "Transform", sub: "↻", nav: "photo-transform", title: "Open Photo Transform Ring", category: "nav" },
  { id: "office-docs-nav", label: "Docs", sub: "•••", nav: "office-docs", title: "Open Office Docs Ring", category: "nav" },
  { id: "office-data-nav", label: "Data", sub: "•••", nav: "office-data", title: "Open Office Data Ring", category: "nav" },
  { id: "video-tools-nav", label: "Tools", sub: "✦", nav: "video-tools", title: "Open Video Tools Ring", category: "nav" },
  { id: "music-more-nav", label: "More", sub: "•••", nav: "music-more", title: "Open More Audio Ring", category: "nav" },
  { id: "music-transcribe-nav", label: "Transcribe", sub: "✦", nav: "music-transcribe", title: "Open Transcribe Ring", category: "nav" },
  { id: "video-transcribe-nav", label: "Subtitles", sub: "✦", nav: "video-transcribe", title: "Open Video Subtitles Ring", category: "nav" },
];

const CATALOG_MAP = new Map<string, CatalogEntry>(CATALOG.map((item) => [item.id, item]));

export function getCatalogItem(id: string): CatalogEntry | undefined {
  return CATALOG_MAP.get(id);
}

// Default slot IDs for each editable ring (excluding the automatic "back" item on sub-rings)
export const DEFAULT_ROOM_SLOTS: Record<RingId, string[]> = {
  // Summary sits right in the lobby: drop a document on it, no room to enter first
  category: IS_MAC ? ["cat-photo", "cat-office", "cat-video", "cat-music", AI_SUMMARY] : ["cat-photo", "cat-office", "cat-video", "cat-music"],
  custom: ["png", "jpg", "webp", "nobg", "pdf", "docx", "xlsx", "mp4"],
  photo: ["jpg", "png", "webp", "heic", "pdf", "photo-formats-nav", "photo-tools-nav"],
  "photo-formats": ["avif", "tiff", "gif", "bmp", "ico", "icns", "photo-pro-nav"],
  "photo-pro": ["jpeg", "tga", "exr", "hdr", "qoi", "ppm", "svg"],
  "photo-tools": ["photo-compress-nav", "nobg", "up-2x", "up-4x", "photo-transform-nav", "square", "gif-anim", "clean"],
  "photo-compress": ["cmp-auto", "cmp-1m", "cmp-500k", "cmp-200k", "cmp-100k", "cmp-lossless"],
  "photo-transform": ["rot-r", "rot-l", "rot-180", "flip-h", "flip-v", "grayscale"],
  office: ["pdf", "docx", "docx-ocr", "xlsx", "pptx", "csv", "office-docs-nav", "office-data-nav"],
  "office-docs": ["md", "txt", "html", "rtf", "epub", "odt"],
  "office-data": ["json", "tsv", "yaml", "xml", "sql", "md-table"],
  video: ["mp4", "mov", "webm", "mkv", "avi", "vid-gif", "video-tools-nav"],
  "video-tools": ["vid-compress", "vid-mute", "vid-mp3", "vid-1080p", "vid-720p", "flv", "video-transcribe-nav"],
  music: ["mp3", "wav", "m4a", "flac", "ogg", "music-more-nav", "music-transcribe-nav"],
  "music-more": ["aiff", "opus", "aac", "caf", "alac", "wma"],
  "music-transcribe": ["tr-srt", "tr-vtt", "tr-txt", "tr-docx", "tr-md"],
  "video-transcribe": ["tr-srt", "tr-vtt", "tr-txt", "tr-docx", "tr-md"],
  size: [],
};

export type WorkspaceConfig = {
  id: string;
  name: string;
  /** "rooms" starts at the Category Lobby; "direct" opens `custom` ring immediately on drag! */
  mode: "rooms" | "direct";
  /** Slot IDs for the `custom` direct ring (3..10 items) */
  customSlots: string[];
  /** Customized slot IDs per room ring (excluding "back", which is appended automatically) */
  roomSlots: Partial<Record<RingId, string[]>>;
  builtin?: boolean;
};

export type WorkspacesState = {
  activeId: string;
  workspaces: WorkspaceConfig[];
};

const STORAGE_KEY = "flipfile.workspaces.v1";

export const DEFAULT_WORKSPACES: WorkspaceConfig[] = [
  {
    id: "all-rooms",
    name: "4 Rooms (Default)",
    mode: "rooms",
    customSlots: [...DEFAULT_ROOM_SLOTS.custom],
    roomSlots: {},
    builtin: true,
  },
  {
    id: "quick-custom",
    name: "My Custom Ring",
    mode: "direct",
    customSlots: ["png", "jpg", "webp", "nobg", "pdf", "docx", "xlsx", "mp4"],
    roomSlots: {},
    builtin: true,
  },
  {
    id: "web-designer",
    name: "Designer & Web",
    mode: "direct",
    customSlots: ["webp", "png", "avif", "svg", "ico", "nobg", "cmp-100k", "square"],
    roomSlots: {},
    builtin: true,
  },
  {
    id: "dev-data",
    name: "Developer & Data",
    mode: "direct",
    customSlots: ["json", "csv", "xlsx", "sql", "yaml", "md", "webp", "ico"],
    roomSlots: {},
    builtin: true,
  },
  {
    id: "office-admin",
    name: "Office & Docs",
    mode: "direct",
    customSlots: ["pdf", "docx", "docx-ocr", "xlsx", "csv", "epub", "jpg", "cmp-1m"],
    roomSlots: {},
    builtin: true,
  },
  {
    id: "creator-media",
    name: "Creator & Media",
    mode: "direct",
    customSlots: ["mp4", "vid-gif", "vid-compress", "vid-1080p", "tr-srt", "mp3", "webp", "nobg"],
    roomSlots: {},
    builtin: true,
  },
];

export function loadWorkspacesState(): WorkspacesState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as WorkspacesState;
      if (parsed && Array.isArray(parsed.workspaces) && parsed.workspaces.length > 0) {
        const hasActive = parsed.workspaces.some((w) => w.id === parsed.activeId);
        return {
          activeId: hasActive ? parsed.activeId : parsed.workspaces[0].id,
          workspaces: parsed.workspaces,
        };
      }
    }
  } catch {
    // ignore corrupt storage
  }
  return {
    activeId: "all-rooms",
    workspaces: DEFAULT_WORKSPACES.map((w) => ({
      ...w,
      customSlots: [...w.customSlots],
      roomSlots: { ...w.roomSlots },
    })),
  };
}

export function saveWorkspacesState(state: WorkspacesState): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // ignore storage quota errors
  }
}

export function getSlotsForRing(ws: WorkspaceConfig, ringId: RingId): string[] {
  if (ringId === "custom") {
    return ws.customSlots.length > 0 ? ws.customSlots : DEFAULT_ROOM_SLOTS.custom;
  }
  const custom = ws.roomSlots[ringId];
  if (custom && custom.length > 0) return custom;
  return defaultSlots(ringId);
}

/** A ring's default slots on this computer (no Summary where it can't run) */
export function defaultSlots(ringId: RingId): string[] {
  const defaults = DEFAULT_ROOM_SLOTS[ringId] ?? [];
  return aiSupport ? defaults : defaults.filter((id) => id !== AI_SUMMARY);
}

function idsToItems(ids: string[]): Item[] {
  const out: Item[] = [];
  for (const id of ids) {
    const entry = CATALOG_MAP.get(id);
    if (entry) {
      out.push({
        id: entry.id,
        label: entry.label,
        sub: entry.sub,
        disabled: entry.disabled || (id === AI_SUMMARY && !aiSupport),
        nav: entry.nav,
        preset: entry.preset,
      });
    }
  }
  return out;
}

const backItem = (target: RingId): Item => ({
  id: "back",
  label: "Back",
  sub: "‹",
  nav: target,
});

export type BuiltWorkspaceRings = {
  rings: Record<RingId, Ring>;
  paths: Record<RingId, string[]>;
  entryRing: RingId;
};

/**
 * Dynamically builds the complete set of Ring geometries and SVG segment paths
 * for the active workspace.
 */
export function buildWorkspaceRings(ws: WorkspaceConfig): BuiltWorkspaceRings {
  const entryRing: RingId = ws.mode === "direct" ? "custom" : "category";

  const categoryItems = idsToItems(getSlotsForRing(ws, "category"));
  const category = buildRing("category", categoryItems.length >= 2 ? categoryItems : BASE_RINGS.category.items);

  const customItems = idsToItems(getSlotsForRing(ws, "custom"));
  const custom = buildRing("custom", customItems.length >= 2 ? customItems : idsToItems(DEFAULT_ROOM_SLOTS.custom));

  // Top-level room rings return to `entryRing` ("category" in rooms mode, "custom" in direct mode)
  const makeTopRoom = (id: RingId): Ring => {
    const items = [...idsToItems(getSlotsForRing(ws, id)), backItem(entryRing)];
    return buildRing(id, items);
  };

  const photo = makeTopRoom("photo");
  const office = makeTopRoom("office");
  const video = makeTopRoom("video");
  const music = makeTopRoom("music");

  // Sub-rings align their "Back" segment with the parent segment that opened them
  const makeSub = (id: RingId, parent: Ring, parentId: RingId): Ring => {
    const items = [...idsToItems(getSlotsForRing(ws, id)), backItem(parentId)];
    return alignedSubRing(id, items, parent, id, parentId);
  };

  const photoFormats = makeSub("photo-formats", photo, "photo");
  const photoPro = makeSub("photo-pro", photoFormats, "photo-formats");
  const photoTools = makeSub("photo-tools", photo, "photo");
  const photoCompress = makeSub("photo-compress", photoTools, "photo-tools");
  const photoTransform = makeSub("photo-transform", photoTools, "photo-tools");

  const officeDocs = makeSub("office-docs", office, "office");
  const officeData = makeSub("office-data", office, "office");
  const videoTools = makeSub("video-tools", video, "video");
  const musicMore = makeSub("music-more", music, "music");
  const musicTranscribe = makeSub("music-transcribe", music, "music");
  const videoTranscribe = makeSub("video-transcribe", videoTools, "video-tools");

  const rings: Record<RingId, Ring> = {
    category,
    custom,
    photo,
    "photo-formats": photoFormats,
    "photo-pro": photoPro,
    "photo-tools": photoTools,
    "photo-compress": photoCompress,
    "photo-transform": photoTransform,
    office,
    "office-docs": officeDocs,
    "office-data": officeData,
    video,
    "video-tools": videoTools,
    music,
    "music-more": musicMore,
    "music-transcribe": musicTranscribe,
    "video-transcribe": videoTranscribe,
    size: BASE_RINGS.size,
  };

  const paths = Object.fromEntries(
    Object.values(rings).map((r) => [r.id, r.items.map((_, i) => segmentPath(r, i))])
  ) as Record<RingId, string[]>;

  return { rings, paths, entryRing };
}

