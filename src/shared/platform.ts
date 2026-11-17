// Which desktop this runs on, from the user agent (avoids pulling in @tauri-apps/plugin-os
// for a few checks). Every platform-specific choice in the UI starts here.

export const IS_MAC = /Mac/i.test(navigator.userAgent);
export const IS_WINDOWS = /Windows/i.test(navigator.userAgent);

/** "This Mac" / "this PC" / "this computer", for where on-device work happens */
export const DEVICE_NAME = IS_MAC ? "Mac" : IS_WINDOWS ? "PC" : "computer";

/** The label for revealing a file in the system file manager */
export const SHOW_IN_FILE_MANAGER = IS_MAC ? "Show in Finder" : IS_WINDOWS ? "Show in Explorer" : "Show";
