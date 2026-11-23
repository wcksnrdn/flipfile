import { CORE_SIZE, type RingId } from "../../rings/dropTarget";
import { DEVICE_NAME, IS_MAC, IS_WINDOWS } from "../../shared/platform";

// Office documents, data tables, and audio/video files convert one by one; only images merge into one PDF
export const OFFICE_FILE =
  /\.(docx|docm|dotx|dotm|doc|dot|xlsx|xlsm|xltx|xltm|xls|xlt|ods|csv|tsv|tab|json|yaml|yml|xml|odt|rtf|md|markdown|txt|text|log|html|htm|epub|pptx|pptm|ppsx|ppsm|potx|potm|ppt|pps|pot|odp|mp3|wav|wave|m4a|flac|ogg|oga|aif|aiff|opus|aac|caf|wma|mp4|m4v|mov|webm|mkv|avi|flv|wmv|mpg|mpeg|ts|mts|3gp)$/i;

// Dropping on a size preset keeps each file's own format (see SAME_FORMAT in convert.rs)
export const SAME_FORMAT = "same";

// If the drop position itself resolves to nothing, a hover this recent still counts.
export const HOVER_GRACE_MS = 150;
// Mouse travel (px) before a press on the core becomes a window drag instead of a click.
export const DRAG_THRESHOLD = 3;

// Progress ring drawn just outside the core
export const PROGRESS_R = CORE_SIZE / 2 + 5;
export const PROGRESS_LEN = 2 * Math.PI * PROGRESS_R;

// How long a finished status stays on the circle before it returns to "Convert"
export const STATUS_CLEAR_MS = {
  result: 6000, // a conversion that produced files (click to show them)
  failure: 3500,
  notice: 3000, // e.g. "Nothing to paste"
  summaryDone: 8000,
  summaryFailed: 5000,
} as const;

// Option/Alt presets: macOS and Windows only (see src-tauri/src/keys.rs)
export const ALT_SUPPORTED = IS_MAC || IS_WINDOWS;
export const ALT_KEY = IS_MAC ? "⌥" : "Alt";
export const ALT_POLL_MS = 80;
// Where the AI tools run: this computer, never a server
export const ON_DEVICE = `On this ${DEVICE_NAME}`;

// Category ring labels for the core center text
export const CATEGORY_LABELS: Partial<Record<RingId, string>> = {
  category: "Convert",
  custom: "Custom",
  photo: "Photo",
  "photo-formats": "Formats",
  "photo-pro": "Pro / 3D",
  "photo-tools": "Tools",
  "photo-compress": "Compress",
  "photo-upscale": "Upscale",
  "photo-transform": "Transform",
  office: "Office",
  "office-docs": "Docs",
  "office-data": "Data",
  video: "Video",
  "video-tools": "Video Tools",
  music: "Music",
  "music-more": "More Audio",
  "music-transcribe": "Transcribe",
  "video-transcribe": "Subtitles",
};
