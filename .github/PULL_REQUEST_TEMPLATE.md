## Summary

<!-- Briefly describe what this Pull Request changes and why. Link any related issue e.g., Closes #123 -->

## Type of Change

- [ ] 🐛 Bug fix (non-breaking change that fixes an issue)
- [ ] ✨ New format or conversion tool
- [ ] 🎨 UI / Ring Workshop / UX improvement
- [ ] ⚡ Performance or memory optimization
- [ ] 🔒 Security hardening
- [ ] 📝 Documentation / CI / governance update

## Privacy, Security & Architecture Checklist

- [ ] **100% Offline Guarantee:** No runtime network requests, telemetry, or external CDN assets were introduced.
- [ ] **Never Overwrites Originals:** Output paths use `src-tauri/src/convert/output.rs` collision-safe naming and clean up partial files on error.
- [ ] **Tauri Least-Privilege ACL:** Any new `#[tauri::command]` is registered in `src-tauri/build.rs` and explicitly permitted in `src-tauri/capabilities/default.json`.
- [ ] **Path Grants Respected:** File operations only touch user-granted paths validated by `src-tauri/src/state/grants.rs`.

## Verification & Testing

- [ ] `npx tsc --noEmit && npm run build` passes with zero errors
- [ ] `cd src-tauri && cargo check` passes with zero warnings/errors
- [ ] `cd src-tauri && cargo test` passes (and new unit tests were added where applicable)
- [ ] Tested interactively in `npm run tauri dev`

## Screenshots / Demo (if UI or document output changed)

<!-- Drag and drop screenshots or before/after comparisons here -->

