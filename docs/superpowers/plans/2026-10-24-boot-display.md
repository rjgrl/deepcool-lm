# Boot Display Implementation Plan

**Goal:** Display the saved layout or media after Windows starts, before sign-in.

**Architecture:** A dedicated `--boot` display worker runs in a noninteractive scheduled task under the installing user's account, with S4U logon and limited privileges. It starts no dashboard or command server. Global Windows events coordinate a graceful handover to the interactive app: finish USB work, release the device without blanking, then let the tray app connect. The interactive app holds the stop event open to prevent a delayed boot task from taking the device back.

**Constraints:** Keep existing Rust standard-library/Windows API architecture. Preserve saved content and sign-in startup. Store no password. No automatic reboot, BIOS changes, service, or automatic Zen toggles. Boot mode does not promise recovery from the earlier hardware state. S4U cannot access network shares or EFS-encrypted media.

**Alternatives:** Reusing `--no-tray` exposes unnecessary servers and provides no handover. A Windows service adds lifecycle/privilege changes and unsupported GDI+ service rendering. A static boot image would discard the user's saved animation/layout before login.

- [x] Add Windows event handover tests: boot yields before user startup returns, duplicate boot is skipped, late boot is skipped, missing completion fails safely.
- [x] Implement the event owner, dedicated boot entry point, logging, and interactive startup handover. Test using native events without touching the LCD.
- [x] Add a focused boot-task setup script, opt-in installer integration, uninstall cleanup, and README instructions. Validate the actual task definition.
- [x] Build and run checks, preserving the known pre-existing image brightness test failure. Review the change for security and shutdown races.
- [x] Install the built executable with a backup, enable sign-in startup, register the startup task, and test the noninteractive worker/handover on this PC. Do not reboot automatically. Report pre-sign-in hardware verification as pending until a real boot is observed.

## Verification on 2026-10-24

- `cargo check` and `cargo build --release` passed. Three native Windows handover tests passed. Full tests: 24 passed; the previously failing `media::tests::clip_fits_and_caches` brightness assertion remains unchanged.
- All three PowerShell scripts parse. Six assertions on the registered task confirmed S4U, limited privileges, boot trigger, `--boot`, unlimited execution time, and IgnoreNew duplicate policy.
- Actual scheduled execution ran in Session 0 at 12:16:08, connected via WinUSB, applied brightness 100, and sent the configured media. It opened no listening TCP sockets.
- Starting the interactive app at 12:17:46 gracefully stopped the boot worker. The task returned 0 and the Session 1 tray app reconnected. Starting the task again while the tray app ran logged that boot display was skipped and returned 0.
- A separate direct USB open was refused while the tray app owned the cooler. The saved configuration hash remained unchanged.
- Review found an installer task-name collision risk; the installer now validates the existing action before disabling/stopping the task.
- The old installed executable is backed up under `target/boot-start-backup/`. The updated executable and enabled startup task are installed; the interactive app was restored.
- The user subsequently confirmed the custom GIF displayed after a real restart while paused at the Windows sign-in screen.
- A later sensor-panel handover test exposed default S4U event permissions denying a normal interactive token. DeepCool's live log confirmed the same Windows error 5 after that real restart. Events now explicitly grant access only to the current account and SYSTEM with a medium integrity label. A regression test using a restrictive thread-token default DACL failed before this fix and passes after it. This does not change device commands or claim to fix the earlier power-reset issue.
- The correction was built and installed with the pre-correction executable backed up as `target/boot-handover-backup.exe`. All four boot tests pass. At 12:51:59 the actual Session 0 worker released the cooler to the normal Session 1 tray with task result 0. A late task skipped at 12:52:00. Config stayed unchanged; boot startup remains enabled. Native FFI and security changes received an independent read-only review with no confirmed actionable findings.
