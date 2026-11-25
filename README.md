# AudioCraft

A native Rust audio editor and multitrack mixer (eframe/egui, no web views). It follows the
ArtCraft guidelines: familiar professional layout and shortcuts, local files only, every feature is a
documented command, MIT licensed. It was built clean-room from public knowledge only.

## Features

- **Waveform editor:** open WAV/MP3/FLAC/OGG/M4A/AIFF, save WAV (8/16/24/32-bit, 32-bit float); cut,
  copy, paste, mix paste, crop, delete and insert silence; unlimited undo with a History panel; markers;
  zoom and scroll; drag-in fade handles with curve control; HUD gain knob; spectral frequency display.
- **Effects:** amplify, 6-band parametric EQ, filters, compressor, hard limiter, noise gate, DeHummer,
  delay, reverb, chorus, flanger, distortion and stereo expander. Effects can run live in the Effects
  Rack (per file or per track) or be applied permanently; its ten slots and Input/Output/Mix strip stay
  visible even before a file is opened. There is also noise reduction from a noise
  print, normalize, fades, match loudness, time-stretch (keeps pitch), pitch shift (keeps length),
  varispeed, tone and noise generators, and favorites.
- **Multitrack:** tracks with volume, pan, mute, solo, record arm, a live effects rack and a level
  meter; clips that can be moved, trimmed, slipped, split (razor), faded and given volume envelopes;
  with the **Move tool** (the black mouse) Shift+click on a clip selects every clip in its row and
  Ctrl+click toggles the one clip; on empty lane space Shift+click selects every empty span of the
  row and Ctrl+click toggles the span clicked; over a clip's top bar these clicks work with every
  tool (the one grab zone they all share). The whole selection is highlighted, not just the clip
  picked last. Holding Shift while dragging locks the move to the main 8 directions (a mostly-upward
  drag moves the clip straight up, a mostly-rightward one straight along its track);
  track and Master **automation lanes**: the chevron beside each Read selector opens a lane *under*
  the track (the track grows), with Show Envelopes (Volume, Pan), Select (which one is edited), and
  protect / clear / previous / add / next keyframe tools; drag the lane's lower border to resize it; the wheel scrolls the tracks, and over the track headers or with Alt held it makes every track taller or shorter (scrolling is a view change and is never logged in the undo history);
  snapping (its magnet is in the Multitrack Tools corner, together with the other
  modifiers; the small arrow beside the magnet opens a regular drop-down menu of targets: clip
  starts and ends, markers, the ruler's time marks, the playhead and the session start; moved clips and trimmed or
  stretched clip edges snap to the other clips' starts and ends on their own track and on the
  tracks above and below, and stay snapped while the drag rests there); the time
  display format (decimal, samples, seconds, SMPTE, CD) is picked at the top of the Selection/View
  panel (the bottom row can't be dragged smaller than that panel needs); a **pre-render gauge**
  along the ruler's top edge, in the theme's colours, whenever a track has Pre-render on (the
  lightning button in Track Effects): filled where the tracks already play from rendered audio,
  faded over the stretch still being rendered (renders run a little every frame; outside the
  rendered stretch a track plays its effects live; hover for the percentage and cache use).
  Pre-render is dynamic (Preferences > Multitrack > Pre-render): pre-rendered tracks share a
  cache cap in MB; when a track's whole render fits its share the whole session is filled,
  otherwise the render follows the playhead, a set number of seconds ahead and behind, dropping
  what falls behind and starting over where the playhead jumps. Each render carries on past
  the track's last clip until its effects have died away (a reverb or delay tail), up to the
  "longest effect tail" setting; a mixer view; session save/load (`.acsession`);
  **library-based media links**: sessions (.acsession and .sesx) store every media file relative
  to the session file (with `../` steps when it sits higher up), so moving or copying the whole
  library folder (the deepest folder holding the session and all its media) anywhere keeps
  every link; the old absolute path is only a fallback. Media that can't be found stays in the
  session as an *Offline* file (silent, its clips kept, its link saved) and **Relink Media**
  (File menu, or the link button in the Files panel) opens: search a folder only or it and every
  folder inside it, ignore case, accept any audio extension, require the file to be long enough
  for its clips, pick among several matches, or locate a file by hand;
  mixdown to a file or to a new file.
- **Preferences that act:** Auto Save writes `name_backup_time.acsession` copies of a changed
  session into a Backups folder beside it every few minutes, keeping the newest few (audio never
  saved is left out of a backup); Data: triangular dither when WAV/AIFF/FLAC is written at 8, 16
  or 24 bits, clipping 32-bit float files above 0 dBFS on open, and a short crossfade at delete
  and cut seams in the Waveform editor; Effects: the default mix of a newly added effect and the
  start-up plug-in scan; Memory: the session history limit; General: how the view follows
  playback (page, centred, off) and opening the last session on start-up; Multitrack: new
  tracks' lane height and Pre-render; Multitrack Clips: automatic crossfades on or off. Media & Disk
  Cache keeps opened files' waveform peaks in a media cache in the temporary folder (oldest go
  past the cache limit), and the temporary folder also holds exports in progress and speech
  takes; Multitrack Clips: the default crossfade curve (equal power, linear, logarithmic),
  smooth (never overshooting) curves through fade points, and "play overlapped portions" (off: a
  clip under a later one is silent there, unless the two are crossfaded); Playback and
  Recording: Play with Pre-roll and Post-roll (Ctrl+Shift+Space), punch-in recording on the
  time selection, and Multitrack playback stopping after the last clip once the sound has died
  away. Each Preferences page says at the top which of its settings are not connected yet
  (now only the control-surface device and parallel effects).
- **Bus tracks:** Multitrack > Track > Add Stereo Bus Track. A track's send (Sends header
  view: on, level, pre/post fader) or its whole output (output menu > Bus) can feed a bus; busses
  are mixed after the tracks that feed them, through their own effects and fader, into the
  Master, and stay audible while a feeding track is soloed. Routes are kept by the bus's id, so
  renaming a bus keeps them. Busses are kept in `.acsession` files (a `.sesx` save writes them
  as plain tracks).
- **Start-up:** AudioCraft no longer loads every VST3/CLAP plug-in when it opens: the list from
  the last scan is shown and each plug-in loads when first used, so opening and closing stay
  fast; the input device's channel count is asked on a background thread.
- **Inputs and outputs:** each track records from any input of the interface, a stereo pair
  ("Stereo In 3/4") or one input ("Mono In 5"), and each armed track gets a take of its own
  input; monitoring uses the same input. Tracks play through the Master, or straight to an
  output pair of their own (output menu > Hardware output > "Out 3/4"; live playback only,
  mixdowns and exports keep every track), or "None". The Master plays on the outputs chosen in
  Preferences > Audio Channel Mapping, which also shows how many channels the devices have.
- **MCP (AI assistants):** Preferences > Control Surface > MCP lets assistants that speak the
  Model Context Protocol work in AudioCraft. Off by default. The running app serves
  `http://127.0.0.1:PORT/mcp` (this computer only, with a secret access token; requests from a web
  page's origin are refused); assistants that start a program use `audiocraft --mcp-stdio`
  ("Copy assistant setup" gives the settings). Tools: get_state, list_tracks, list_commands,
  run_command, set_playhead, set_selection, set_track. Guardrails: four permission levels (Read
  only, Navigate & play, Edit, Full), "Ask me before each change" (Allow / Deny in the app,
  declined after a minute), a requests-per-minute limit, an activity log, every edit undoable;
  quitting, anything that opens a dialog or another program, plug-in scans and choosing where
  files are saved are never available. Protocol: MCP 2025-11-25 (also 2025-06-18, 2025-03-26 and
  2024-11-05); `initialize` gives an `Mcp-Session-Id` that later requests send back with the
  `MCP-Protocol-Version` header (unknown session: 404, unknown version: 400, DELETE ends a
  session; batches only for 2025-03-26 and older). Tools carry titles and read-only /
  destructive / idempotent hints; get_state, list_tracks and list_commands also return
  `structuredContent` matching their `outputSchema`. The stdio bridge re-reads the preferences
  for each message, so turning MCP on or changing the port needs no assistant restart.
- **Keyframes:** every envelope's keyframes (clip volume and pan, track and Master lanes, fade
  points) are small squares that light up under the pointer, select on click (Shift/Ctrl adds) in
  the selection colour, drag (together when several are selected; Shift locks the move to the main
  8 directions) and go away with Delete or a double-click. Click a line to add one. The **Direct
  Selection tool** (white arrow, `A`) shows the
  curve handles: drag a handle to bend the curve (its partner turns with it), Alt+drag a handle to
  move it alone, Alt+drag a keyframe to pull out new symmetric handles, Alt+click a keyframe to give
  it handles or take them away. Sizes and the selection colour are in Preferences > Multitrack.
- **Fades and crossfades** can be shaped: click a fade or crossfade line to add a point, drag it,
  double-click to take it out; the curve runs smoothly through the points and plays as drawn.
- **Audition compatibility:** saving a `.sesx` checks for things the format cannot hold (curved
  keyframes, shaped fades) and offers to convert them (they become ordinary keyframes, so the
  session sounds the same in Audition), save without them, preview the conversion first (one Undo
  takes it back), or save an `.acsession` that keeps everything. The message can be turned off in
  Preferences > Multitrack.
- **Preferences** (Edit > Preferences, `Ctrl+Shift+K`): General, Appearance, Audio Channel Mapping,
  Audio Hardware, Auto Save, Control Surface, Data, Effects, Markers & Metadata, Media & Disk Cache,
  Memory, Multitrack, Multitrack Clips, Playback and Recording, Spectral Displays, Time Display and
  Video, with a search field; saved to `preferences.json` beside the workspaces. Appearance offers
  24 colour themes (AudioCraft, the darker Audition Dark, and the PhotoCraft and VectorCraft
  themes), and Multitrack sets the height of a fresh track's Input and Output buttons. Themes are picked from preview cards (dark and light rows, as in PhotoCraft). Every
  change shows at once while the window is open; OK keeps it, Cancel or Esc puts the old settings
  back (the interface size applies when the mouse button is let go).
- **More of the original's menus:** Edit > Set Current Clipboard (five clipboards), Multitrack >
  Metronome (enable, beats per bar, tempo), Clip > Clip Gain, Bring to Front and Remove Fade In/Out.
  The Effects Rack has sixteen slots. Preferences > Video > Preview resolution picks Fit to panel,
  720p, 1080p or Native for the video picture.
- **Tracks panel:** shows which tracks the Editor displays. The eye at the left of each row
  hides or shows a track (hidden tracks still play); Show All, Hide All, Only Selected and Hide
  Empty are buttons there and in Multitrack > Track Visibility.
- **Speech:** the system voice speaks through the Windows SAPI engine with every installed
  voice (classic and OneCore). Ready voices pick a different real voice each (Woman, Woman 2,
  Man, ...); Pitch uses the engine's own pitch control (about ±4.5 semitones, the voice keeps its
  character). Preview plays on the separate preview player (like the Files panel): the playhead
  and tracks don't move, and the button turns back to Play when the speech ends. The Vocal Tract
  (Pink Trombone model) runs at its tuned 48 kHz with the tongue shaped as in the model, so vowels
  have their own resonances; it has ready voices, Voice size (formants), Whisper, English
  spelling rules and a rising pitch for questions.
- **Sliders:** double-click a slider or its name to put its default value back.
- **Appearance:** Brightness lightens or darkens every surface; Saturation makes all colours
  (track colours included) more vivid or greyer.
- **Drag toggles:** press an effect's power switch, a Tracks panel eye or a track's minimise
  button and drag across the others: they all take the new state of the first one.
- **Spectral displays:** the Spectral Frequency Display has a linear or logarithmic scale
  (right-click its scale), wheel zoom around the pointer, drag to scroll, right-drag to zoom to a
  range; a moved spectral selection keeps its height on screen (or its Hz width: Preferences >
  Spectral Displays). The Spectral Pitch Display is its own pane with note names on the scale, the
  harmonics in red-to-gold and the detected pitch as a blue line; drag the borders between the
  waveform and the displays to share the editor between them.
- **Files panel:** play the selected file, loop it, or auto-play files as you pick them.
- **Recording** from any input device, into a file or onto armed tracks.
- **Analysis:** level meters, amplitude statistics, frequency analysis, and a CSV analysis report
  (peak, RMS and loudness estimate) for the current file.
- **Essential Sound:** one-click loudness, repair (noise, rumble, hum), clarity, width and reverb.
- **Workflows:** marker navigation and CSV import/export; play-selection and return-to-start transport
  commands; Gain and Repair Clipped Peaks; clip duplicate/fade/crossfade and split-at-markers; track
  duplicate/rename/mute-all and clear-solos; spectral attenuate and select-all; and a **Batch Processor**
  that applies a preset to many files and writes them into an output folder.
- **Open and Import:** the file picker leads with **Session and AudioCraft files**
  (`.sesx`, `.acsession`, `.audiocraft`), followed by audio files and All files. Dragging a session
  opens it as a session; dragging a video always goes to the Video panel, never onto an audio lane.
- **Video:** **File > Import Video File...** (`Ctrl+Shift+I`) or drag/drop opens MP4/MOV/MKV/AVI/WEBM
  and other common containers. Import **streams**: the clip's size shows in a few hundredths of a
  second, its sound is decoded straight from the decoder's pipe while preview thumbnails arrive in
  batches (long clips decode key frames only), and no temporary files are written. A 20-minute 720p
  test clip imports in about half a second. When the playhead stops, the exact frame under it is
  decoded at a sharper size. The decoder never opens a console window. A picture that is not
  standard Rec.709 8-bit (HDR, BT.2020, 10-bit) brings up a **Video Color Space** window that can
  convert the preview to Rec.709. The file is labelled as video (with its real
  container in the status bar), never presented as a WAV. A free FFmpeg decoder is **bundled**
  (`vendor/ffmpeg/bin`), so decoding works all the time without installing anything; if the Video
  panel shows an error slate, that copy is missing from the installation. To use a different build,
  pick it in **Preferences > Video**: the list shows every decoder found (bundled, downloaded, the
  system's) and which one is used. **Download a free decoder** fetches a free FFmpeg build (BtbN
  LGPL or GPL, gyan.dev essentials) with the system's `curl`/`tar` into AudioCraft's decoders folder
  and selects it; nothing is downloaded unless you press it. The same decoder also writes MP3/AAC
  exports.
- **Recording like a tape machine:** arm a track (R) and pick its input, press **Record** (it
  blinks: ready), then **Play** starts recording; Record while playing punches in. Nothing records
  without a session and an armed track (Multitrack) or an open file (Waveform; Record offers a new
  file). Armed and monitored (I) tracks show their input on their meters and in Levels before and
  during recording; **I** plays the input through the output. The take is drawn live, growing on
  every armed track (and in the Waveform editor), while it records.
- **Generate > Speech** has two tabs: **System Voice** (Windows voices, espeak/flite elsewhere) and
  **Vocal Tract**, a physical voice model (glottis, tract and nose) with tongue/constriction pads,
  pitch, tenseness, vibrato and a "Say" line. Preview plays without adding a file; **Add as New
  File** keeps the window open; speaking runs on a worker.
- **Windows of panels:** Window > **New Window** opens a window for panels (Add Panel menu); a
  panel's menu has **Undock Panel / Undock Panel Group**; closing such a window docks its panels
  back. The **Maximum Editing (Dual Monitor)** workspace puts the panels in a second window for the
  other monitor.
- **Search Help** (toolbar): results drop down while typing; pick one to run it or press ? for its
  help; the Help window lists every command with its shortcut and description.
- **Batch Processor** runs on a worker with a per-file status (Waiting / Processing / Done /
  Error), accepts dropped files and "Add Open Files", and writes into Documents/AudioCraft/Batch by
  default (Open Output Folder shows it).
- **Files panel** columns resize by dragging the header dividers (double-click the Name divider to
  fit the names); the list scrolls sideways when wider than the panel; hover a file for its path.
- **Waveform fade handles** stay where they are let go and can be dragged again (one undo step
  holds both fades); dragged far they stay at the edge instead of vanishing.

- **Effects Rack** holds up to 64 effects: it always shows ten slots and grows past them, keeping
  one empty slot ready for the next effect.
- **Time Display** (View > Time Display, or click the big time): Decimal, Samples, Seconds, SMPTE
  timecode (Preferences > Time Display sets the frames per second) and Compact Disc (75 fps).
  View > Status Bar picks what the status bar shows. Multitrack > Track > Move Track Up/Down,
  Clip > Match Clip Loudness (every clip on the selected track to the same heard level),
  Help > Open Settings Folder.
- **One look for every control:** square-cornered, rectangular panels, buttons, menus and
  dialogs; dark popup menus with a quiet grey highlight (no blue blocks), check marks and radio
  dots in a left gutter; solid blue when on, green power switches, the default dialog button blue.
  View > Next Colour Theme steps through the colour themes; the
  application keeps its AudioCraft name. Each dialog sizes to its own content (they no longer
  stretch to the window's full size).

## Starting a project

With an empty project, the **Waveform** and **Multitrack** buttons open the New Audio File and
New Multitrack Session dialogs; once a file or session exists they simply switch views. Opening
the **Mixer** (tab, panel or command) with no session instantly creates a ready six-track session.

## Commands

Every action has an ID (see [docs/COMMANDS.md](docs/COMMANDS.md)). You can run it from:

- the menus (spectral displays in **View**, analysis panels in **Window** or **Effects**)
  and the keyboard shortcuts,
- the **Command Palette** (`Ctrl+Shift+P`),
- the command line:

```
audiocraft --list-commands
audiocraft --batch in.wav --do process.normalize=-1 --do favorite.podcast_voice --out out.wav
audiocraft --help
```

The Batch Processor (`File > Batch Processor...`) runs the same presets across a list of files
from the UI, writing `name_suffix.wav` into a chosen output folder; one bad file does not stop the rest.

For checking the effect editors, `--screenshot` can open one panel by name:
`audiocraft --screenshot out.png --fxshot "Single-band Compressor"`. The creation and close prompts
can be checked with `--shot-dialog file`, `session`, or `close` (use `--demo` for a populated close
prompt). Add `--autoshot`
to a `--demo --multitrack` screenshot to open the volume automation lanes with example points.
`AUDIOCRAFT_SHOT_PLUGIN=Name audiocraft --screenshot out.png --demo` opens that installed
VST3/CLAP effect in the Plug-ins panel and also saves `out-native.png`, a capture of the window
with its child windows (plug-ins that draw with the GPU may not appear in it).

## Plug-ins panel

**Window > Plug-ins** shows or hides the Plug-ins panel (it docks in its own column at the right
edge). VST3 and CLAP effects open *their own* editor windows there instead of the generic
sliders: insert one in the Effects Rack, or click one that is already there. Each open editor has
a tab (× or a middle-click closes it); the strip under the tabs bypasses the effect, shows which
rack and slot it is in, and offers **Pop Out** (the editor moves into its own floating window and
keeps its tab; **Dock in Panel** brings it back) and **Generic Controls** (the slider window). The
column widens to fit a new editor, the editor scrolls when the column is narrower, and it steps
aside while a menu or dialog covers it. The panel's ≡ menu has **Close All Plug-in Windows**,
**Dock Plug-in Windows in the Panel (on/off)**, the **Audio Plug-In Manager**, and **Close
Plug-ins Panel** (as does the generic **Close Panel**). Closing the panel only hides it: the
editors stay open and come back with it. Docking is a Windows feature; elsewhere (or with
Preferences > Effects > *Open VST3 and CLAP plug-in windows in the Plug-ins panel* turned off)
editors float in their own windows and the panel lists them. CLAP plug-ins that only offer a
floating window keep it.

A plug-in's window drives the very instance that is playing, so turning its knobs changes the
sound at once, also during playback (only a bypassed effect, or one in a rack that is not
playing now, opens on a separate copy whose settings are copied over). After an editor opens,
or when you click back into AudioCraft, the keyboard returns to AudioCraft; Space plays and
stops even while a plug-in window has the keyboard.

Plug-ins are created and switched on by the interface thread (some stay silent when the audio
thread does it), prepared once for stereo, and kept running: a turned knob, a newly saved state,
or another effect being added or bypassed updates the running instance instead of rebuilding it,
so edits are heard while playing. `audiocraft --probe-plugin Name` runs an installed effect the
way playback does and prints its levels before and after a live edit (a diagnostic).

## Build and test

```
cargo build
cargo test --target-dir target-alt -- --test-threads=1
```

Besides unit tests, `src/tests_app.rs` drives the whole app frame by frame (no window, sound card,
native dialog or plug-in): every command with nothing open, with a file, in a session and on odd
audio (empty, one sample, eight channels, NaN samples); every workspace and panel at three window
sizes; clicks across the whole window; and every effect and process on reversed or out-of-range
selections. Tests keep their settings in a temporary folder, never the person's own.
`AUDIOCRAFT_SHOT_CLICKS="frame:x,y;frame:x,y:h"` makes a `--screenshot` run click (or `:h` hover)
at those points (in interface points, i.e. pixels divided by the display scale), e.g. to capture
an open menu.

## Linux build

The same source builds on Linux (x86_64). On a Debian/Ubuntu machine:

```
bash scripts/build-linux.sh
```

The script installs the required system libraries (ALSA, X11/Wayland, GTK, fontconfig and OpenGL
headers), builds a release binary with `cargo build --release --locked`, and leaves a ready-to-run
copy at `dist/audiocraft-linux-x86_64` plus a `dist/audiocraft-linux-x86_64.tar.gz` archive.
Fedora and Arch are supported too; pass `--no-deps` when the libraries are already installed.
On Linux the status bar reads free disk space, RAM and CPU time from the system and `/proc`.
Files dragged in from a file manager need X11: under a Wayland desktop AudioCraft opens through
XWayland so drag and drop works (`AUDIOCRAFT_WAYLAND=1` runs it as a native Wayland window, which
cannot receive files dragged in).

### From Windows (no Linux machine needed)

```
rustup target add x86_64-unknown-linux-gnu
python -m pip install --user ziglang
cargo install cargo-zigbuild
python scripts/build-linux-cross.py
```

This cross-builds the same Linux program into `dist/linux/` (program, icon, desktop entry, README,
licence and `audiocraft-linux-x86_64.tar.gz`). Zig links it against glibc 2.28, so it runs on
Linux desktops from 2018 on (Ubuntu 20.04+, Debian 10+, Fedora 29+). It needs only the ALSA
sound library (`libasound.so.2`, on every desktop install) and loads X11/Wayland/OpenGL when it
starts. For the link step the script builds a stand-in for ALSA that is never shipped. Unpack
with `tar xzf audiocraft-linux-x86_64.tar.gz` and run `./audiocraft-linux-x86_64/audiocraft-linux-x86_64`.
Generate Speech uses `espeak` or `flite` from the distribution's packages.

For a binary without installing anything locally, push this folder to GitHub: the
`Linux build` workflow (`build-linux.yml`) compiles it on Ubuntu and uploads
`audiocraft-linux-x86_64` as a downloadable build artifact. The `--screenshot out.png` check works
on Linux as well; its window opens unfocused and is not lowered there.

Tests never touch the plug-ins installed on your machine. Environment switches:

- `AUDIOCRAFT_NO_PLUGIN_SCAN=1`: don't scan or load CLAP/VST3 plug-ins.
- `AUDIOCRAFT_PLUGIN_PATH`: scan only these folders (separated like `PATH`) instead of the
  standard CLAP and VST3 folders.
- `AUDIOCRAFT_NO_BUNDLED_FFMPEG=1`: ignore the bundled decoder (diagnostics/tests only; PATH
  still works).

## Reused open-source code

`vendor/` holds three crates from **SoundCraft** (ArtCraft, MIT OR Apache-2.0; see
`vendor/SOUNDCRAFT-LICENSE-MIT`, `vendor/SOUNDCRAFT-LICENSE-APACHE`, `vendor/SOUNDCRAFT-NOTICE`
and `vendor/SOUNDCRAFT-ATTRIBUTION.md`): `soundcraft-dsp` (built-in effects, meters),
`soundcraft-clap-host` and `soundcraft-vst3-host` (third-party plug-in hosting). AudioCraft
changed them only to add `set_search_paths` (injectable plug-in folders). `vendor/ffmpeg` holds a
separate, unmodified LGPL-licensed FFmpeg executable used to decode video and encode MP3/AAC; see
`vendor/ffmpeg/NOTICE.md` for its exact build, source link and licence texts. `cargo build` copies it
beside `audiocraft.exe`, so the decoder travels with the app.

The Vocal Tract speech model (`src/tract.rs`) is AudioCraft's own implementation of the published
physical model popularised by **Pink Trombone** by Neil Thapen (MIT licence): an LF glottal source
(Fant, Liljencrants and Lin, shaped by Rd) driving a Kelly-Lochbaum waveguide tract with a nasal
branch. Credit for the model and its parameter choices goes to that work; no code was copied.
Decoders downloaded from Preferences > Video come from their maintainers (BtbN FFmpeg-Builds,
gyan.dev) under their own LGPL/GPL licences and are installed beside, not into, AudioCraft.

## Licence and third-party material

AudioCraft is MIT licensed (see `LICENSE`). All icons are drawn in code; no image assets are bundled.
The extra colour themes take their colour values from **PhotoCraft** (© 2026 ArtCraft Team and the
PhotoCraft contributors) and **VectorCraft** 0.24.1 (© 2026 ArtCraft Team and the VectorCraft
contributors), both licensed MIT OR Apache-2.0; PhotoCraft's Solarized Dark uses Ethan Schoonover's
Solarized palette (MIT).
Several line icons (folder-open, file-plus, trash-2, rotate-cw, arrow-left-right) are **Lucide**
icons (ISC licence, © Lucide Icons and Contributors, as shipped with VectorCraft); the licence text
is `vendor/LICENSE-lucide.txt`. The other icons are AudioCraft's own drawings on the same grid.
egui bundles its default fonts (Ubuntu-Light under the Ubuntu Font Licence; Noto Emoji and
emoji-icon-font under the SIL Open Font License / Apache-2.0). Rust dependencies keep their own
licences (MIT and/or Apache-2.0; symphonia is MPL-2.0). The bundled `vendor/ffmpeg/bin/ffmpeg.exe`
is **FFmpeg (LGPL-3.0-or-later)**, © the FFmpeg developers; its licence texts are
`vendor/ffmpeg/LICENSE.txt` and `vendor/ffmpeg/COPYING.GPLv3.txt`, and its source and build details
are in `vendor/ffmpeg/NOTICE.md`. It is started as a separate process, not linked into AudioCraft.
