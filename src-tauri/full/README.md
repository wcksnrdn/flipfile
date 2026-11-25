# Builds that come with every model

Release builds bundle no AI models: people download the ones they want as packs from
Workshop (`src/packs`). These configs bundle them all again, for the bridge release only
(`.github/workflows/release.yml`): the update the apps from before packs get, which copies
its models into the app data folder at start so that the next update, without them, keeps
them.

    FLIPFILE_FULL=1 npm run tauri build -- --config src-tauri/full/coreml.json   # macOS
    FLIPFILE_FULL=1 npm run tauri build -- --config src-tauri/full/onnx.json     # Windows, Linux
