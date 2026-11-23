# Code review: findings (2026-11-22)

## Found and fixed

| What | Why it mattered | What was done |
|---|---|---|
| Plug-in windows could stay tied to a plug-in copy the engine had replaced (playback starting at another sample rate, a rebuilt rack) | Turning a plug-in's knobs during playback changed nothing you could hear | Each window checks every frame which copy is playing and reconnects to it, carrying its settings over (`plugdock::plug_reattach`) |
| About 150 fixed grey colours in the effect editors, meters, mixer, rack, lanes and dialogs | Those parts ignored the chosen theme | They go through `theme::gray`, which maps each grey onto the theme's own surfaces and text; AudioCraft's own theme keeps the exact old greys |
| Title bar always the system colour | The window frame ignored the theme | Windows 11 caption, text and border take the theme's colours (`theme::apply_title_bar`) |
| Dialog buttons in three different styles (small egui buttons, outlined, filled) and two sizes | Some Cancel buttons were bigger and outlined | One flat button for every dialog footer; the main action is the blue one at the far right |
| Save Changes: Yes / Yes To All / No / No To All / Cancel in one row | Discarding sat next to saving | Don't Save (and Don't Save Any) on the left; Cancel, Save All and Save on the right |
| Some theme text colours were too faint | Hard to read in a few PhotoCraft/VectorCraft themes | Text is lifted to at least 4.5:1 and dim text to 3:1; themes under 7:1 are marked "Lower contrast" on their card and in Appearance |
| Preferences > Video "Other:" radio did nothing | Clicking it gave no feedback | It now puts the cursor in the path box |
| Three file pickers called the system dialog directly | A test touching them would hang on a real dialog | They go through the test-safe wrapper (`sysui::Ask`) like the others |
| Two menu-item helpers did the same thing | Duplicate code; the new one lacked shortcuts and help text | The new menus use the main menu helper |
| An unused older copy of the Effects menu (`effects_menu_v1`, 171 lines) | Dead duplicate code | Removed |
| Two unused theme functions | Dead code | Used: the status bar names the new theme; Appearance warns about lower-contrast themes |
| Esc was taken by "clear selection" before any window was drawn | Esc did not cancel dialogs (unless a text box had focus), nor a dragged rack effect | Esc now closes the topmost popup first: a dialog (as Cancel), the effect settings window, a track EQ, an analysis window, Help; it clears the selection only when nothing is open |
| Saving a session wrote changed audio as "<name>.wav" | Two sources with the same name overwrote each other, and an unrelated file of that name in the folder was overwritten | A free name is chosen ("take (2).wav"); tested |
| A noise print taken at one sample rate was used bin-for-bin at another | Noise Reduction removed the wrong frequencies (and an empty print could crash it) | The print is read by frequency; an empty print does nothing |
| Brightness at full turned dark themes grey | It moved surfaces 70% toward white | Brightness is now a trim (a fifth toward white, a quarter toward black): dark themes stay dark, light themes stay light; tested for every theme |
| Unused code | Compiler warnings, dead weight | Removed: unused imports, the unused "fx" path icon, two slider methods, three icons, the "not available" menu item kind, the old left-tab state, unused panel flags, the close-prompt flag, a drag flag and drag kind, two import-queue helpers, the mixer's duplicate pan, a plug-in rate field and the editor's duplicate slot/id; test-only and Linux-only code is marked as such. Only the library's "renamed" notes remain. |

## Still to look at

- **Deprecated `egui::Id::new` calls:** a mechanical change to `Id::unique` or `IdSalt::new`; not urgent.
- **Coloured (non-grey) fixed colours:** blues, reds and greens for selections, record and solo still use AudioCraft's colours in every theme. They read well everywhere, but don't take a theme's accent.
- **Meters stay dark in light themes:** a deliberate choice (meter colours need a dark well); could get a light variant.
- **Plug-in reconnect:** it can't be tried here (tests never load real plug-ins), so it needs a check with your own VST3s during playback.
- **Clip grouping:** clips select, move and delete together. Grouped clips don't show a group mark yet, and trimming one doesn't trim the others.
