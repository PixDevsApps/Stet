## What this changes

<!-- What and why, in a few lines. Link the issue: "Fixes #123". -->

## How it was checked

<!-- Which of these you ran, and on what system. "Not run" is fine; say so. -->

- [ ] `bash tools/ci.sh`
- [ ] `bash tools/ci.sh --ui` (changes in the window)
- [ ] Tried by hand:

Not covered by the headless runs, if it matters here: Wayland input methods, fractional scaling, Hyprland behaviour.

## Before you send it

- [ ] Larger changes were agreed in an issue first ([CONTRIBUTING.md](https://github.com/PixDevsApps/Stet/blob/main/CONTRIBUTING.md))
- [ ] Tests come with the change (a `.stet-test` script for UI behaviour)
- [ ] The documents it affects are updated, with a line in RELEASE_NOTES.md under "Unreleased" for anything a user would notice
- [ ] No private text in tests, screenshots or logs
