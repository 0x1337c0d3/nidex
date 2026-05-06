# ASCII Animation System

## Overview

The Codex TUI displays a spinning ASCII art logo animation on the login/welcome screen when the user is not authenticated. This feature draws inspiration from retro gaming aesthetics and provides visual feedback during the onboarding flow.

## Architecture

### Core Components

1. **Frame Data** (`tui/src/frames.rs`)
   - Embeds 36 animation frames at compile time using `include_str!` macros
   - Supports 10 animation variants, each with 36 frames:
     - `FRAMES_DEFAULT`
     - `FRAMES_CODEX`
     - `FRAMES_OPENAI`
     - `FRAMES_BLOCKS`
     - `FRAMES_DOTS`
     - `FRAMES_HASH`
     - `FRAMES_HBARS`
     - `FRAMES_VBARS`
     - `FRAMES_SHAPES`
     - `FRAMES_SLUG`
   - Frame files are stored in `tui/frames/{variant}/frame_{1-36}.txt`
   - Default frame duration: 80ms per frame

2. **Animation Controller** (`tui/src/ascii_animation.rs`)
   - `AsciiAnimation` struct manages animation state and timing
   - Calculates which frame to display based on elapsed time
   - Supports variant selection (user can press Ctrl+. to randomize)
   - Schedules frame redraws via `FrameRequester`

3. **Welcome Widget** (`tui/src/onboarding/welcome.rs`)
   - `WelcomeWidget` renders the animation in the login flow
   - Only displays when user is not logged in (`is_logged_in: false`)
   - Shows animation above "Welcome to Codex" text if viewport is large enough
   - Minimum dimensions: 60 columns × 37 rows
   - Hidden on smaller terminals to avoid clipping

## Usage

### Keyboard Interaction

Press **Ctrl+.** (Ctrl+Dot) on the welcome screen to cycle through animation variants.

### Current Login Context

**Note:** With the removal of ChatGPT OAuth and the adoption of API key-only authentication, the welcome animation is displayed less frequently but remains part of the onboarding experience.

## Future Modifications

### Removal
To remove the animation entirely:
1. Remove `AsciiAnimation` from `WelcomeWidget::new()`
2. Simplify `WelcomeWidget::render_ref()` to skip frame rendering
3. Delete `tui/src/ascii_animation.rs`
4. Delete `tui/src/frames.rs` and `tui/frames/` directory
5. Update `tui/src/lib.rs` or module declarations

### Customization
- Add new animation variants by creating frame files in `tui/frames/{new_variant}/`
- Adjust frame duration by modifying `FRAME_TICK_DEFAULT` in `frames.rs`
- Change animation selection logic in `AsciiAnimation::pick_random_variant()`

### Performance
- Frames are embedded at compile time, so there's no runtime I/O cost
- Frame cycling is O(1) based on elapsed time and frame count
- Animation only redraws when scheduled by the event loop

## References

- Implementation: `tui/src/onboarding/welcome.rs` (lines 68-96)
- Animation logic: `tui/src/ascii_animation.rs`
- Frame data: `tui/src/frames.rs`
- Frame files: `tui/frames/*/`
