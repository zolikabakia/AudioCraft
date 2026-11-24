//! Command registry: every user-facing action has a stable ID, title, shortcut and doc string.
//! The menus, the command palette (Ctrl+Shift+P) and the command line all run commands by ID.

use crate::app::{App, Dialog, Mode};
use crate::audio::{self, AudioData};
use crate::dsp::{self, Effect, EffectSlot, FadeCurve, FilterKind, NoiseColor, Wave};
use crate::sysui::Ask;

pub struct Cmd {
    pub id: &'static str,
    pub title: &'static str,
    pub shortcut: &'static str,
    pub doc: &'static str,
}

macro_rules! cmds {
    ($(($id:expr, $title:expr, $sc:expr, $doc:expr)),* $(,)?) => {
        &[$(Cmd { id: $id, title: $title, shortcut: $sc, doc: $doc }),*]
    };
}

pub const COMMANDS: &[Cmd] = cmds![
    (
        "file.new",
        "New Audio File",
        "Ctrl+N",
        "Create an empty audio file (asks for sample rate, channels and bit depth)."
    ),
    (
        "file.new_session",
        "New Multitrack Session",
        "Ctrl+Shift+N",
        "Create an empty multitrack session."
    ),
    (
        "file.open",
        "Open",
        "Ctrl+O",
        "Open a session or AudioCraft project first; audio files and videos are also available."
    ),
    (
        "file.import_video",
        "Import Video File...",
        "Ctrl+Shift+I",
        "Import a video in the background, show its frames in the Video panel and play its audio."
    ),
    (
        "file.import_folder",
        "Import Folder...",
        "",
        "Import every audio file, video and session in a folder and its sub-folders (dragging a folder in does the same)."
    ),
    (
        "video.detect_decoder",
        "Detect Video Decoder",
        "",
        "Find the bundled free FFmpeg decoder, or one in the configured folders and the system PATH."
    ),
    (
        "file.save",
        "Save",
        "Ctrl+S",
        "Save the current file as WAV, or the session in Multitrack."
    ),
    (
        "file.save_as",
        "Save As",
        "Ctrl+Shift+S",
        "Save under a new name."
    ),
    (
        "file.close",
        "Close File",
        "Ctrl+W",
        "Close the current file."
    ),
    (
        "file.export_mixdown",
        "Export Multitrack Mixdown...",
        "",
        "Export the whole session as one audio file, with format and sample type choices."
    ),
    (
        "file.batch_process",
        "Batch Processor...",
        "",
        "Run a preset over many files and write the results into an output folder."
    ),
    (
        "file.export_report",
        "Export Analysis Report...",
        "",
        "Save peak, RMS and loudness estimates of the current file as CSV."
    ),
    (
        "edit.undo",
        "Undo",
        "Ctrl+Z",
        "Undo the last change of the current file or session."
    ),
    (
        "edit.redo",
        "Redo",
        "Ctrl+Shift+Z",
        "Redo the last undone change."
    ),
    (
        "edit.cut",
        "Cut",
        "Ctrl+X",
        "Cut the selection to the clipboard."
    ),
    (
        "edit.copy",
        "Copy",
        "Ctrl+C",
        "Copy the selection to the clipboard."
    ),
    (
        "edit.paste",
        "Paste",
        "Ctrl+V",
        "Insert the clipboard at the playhead (replaces the selection)."
    ),
    (
        "edit.mix_paste",
        "Mix Paste",
        "Ctrl+Shift+V",
        "Mix the clipboard over the audio at the playhead."
    ),
    (
        "edit.paste_new",
        "Paste to New File",
        "Ctrl+Alt+V",
        "Create a new file from the clipboard."
    ),
    (
        "edit.delete",
        "Delete",
        "Del",
        "Delete the selection. In Multitrack every selected clip goes and the space it occupied stays; selected empty space closes up instead (there is nothing in it to remove)."
    ),
    (
        "edit.ripple_delete",
        "Ripple Delete",
        "Shift+Del",
        "Delete the selected clips and close the gap they leave: the later clips of their track pull left. Selected empty space closes the same way, whichever key pressed it."
    ),
    ("edit.crop", "Crop", "Ctrl+T", "Keep only the selection."),
    (
        "edit.select_all",
        "Select All",
        "Ctrl+A",
        "Select the whole file or session."
    ),
    (
        "edit.deselect",
        "Deselect All",
        "Ctrl+Shift+A",
        "Clear the selection."
    ),
    (
        "edit.insert_silence",
        "Insert Silence",
        "",
        "Insert silence at the playhead."
    ),
    (
        "edit.convert",
        "Convert Sample Type",
        "Shift+T",
        "Change sample rate, channels or bit depth."
    ),
    (
        "edit.preferences",
        "Preferences",
        "Ctrl+Shift+K",
        "The Preferences window: General, Appearance, Audio Hardware, Multitrack (keyframe and handle sizes), Spectral Displays and more."
    ),
    (
        "tool.direct_selection",
        "Direct Selection Tool",
        "A",
        "The white arrow: keyframes show their curve handles. Drag a handle to bend the curve, Alt+drag a handle to move it alone, Alt+drag a keyframe to pull out new handles, Alt+click a keyframe to add or remove its handles."
    ),
    (
        "keyframes.delete",
        "Delete Selected Keyframes",
        "Del",
        "Remove the selected keyframes (click a keyframe to select it; Shift or Ctrl adds to the selection)."
    ),
    (
        "keyframes.deselect",
        "Deselect Keyframes",
        "Esc",
        "Clear the keyframe selection."
    ),
    (
        "session.convert_for_audition",
        "Convert Curves for Audition",
        "",
        "Turn curved keyframes into ordinary keyframes and shaped fades into the clip volume envelope, so a .sesx file holds the session exactly (one Undo takes it back)."
    ),
    (
        "view.spectral_log",
        "Logarithmic Spectral Scale",
        "",
        "Switch the Spectral Frequency Display between a linear and a logarithmic frequency scale."
    ),
    (
        "view.spectral_pitch",
        "Show Spectral Pitch Display",
        "",
        "Show or hide the Spectral Pitch Display (notes on the scale, the detected pitch as a blue line)."
    ),
    (
        "files.play",
        "Play Selected File",
        "",
        "Play (or stop) the selected file in the Files panel's own preview player; never moves the editor playhead."
    ),
    (
        "files.loop",
        "Loop File Playback",
        "",
        "Loop the Files panel's independent preview player."
    ),
    (
        "files.autoplay",
        "Auto-Play Files",
        "",
        "Play each file in the file browser as it is selected, without moving the playhead."
    ),
    (
        "marker.add",
        "Add Marker",
        "M",
        "Add a marker at the playhead (or a range marker for the selection)."
    ),
    (
        "marker.next",
        "Go to Next Marker",
        "PgDn",
        "Move the playhead to the next marker (wraps around)."
    ),
    (
        "marker.previous",
        "Go to Previous Marker",
        "PgUp",
        "Move the playhead to the previous marker (wraps around)."
    ),
    (
        "marker.rename",
        "Rename Marker...",
        "",
        "Rename the marker nearest the playhead."
    ),
    (
        "marker.delete",
        "Delete Marker",
        "",
        "Delete the marker nearest the playhead."
    ),
    (
        "marker.export",
        "Export Markers...",
        "",
        "Save markers as a small CSV file (name, start, duration)."
    ),
    (
        "marker.import",
        "Import Markers...",
        "",
        "Add markers from a CSV file with name, start and duration columns."
    ),
    (
        "transport.play",
        "Play / Stop",
        "Space",
        "Start or stop playback."
    ),
    (
        "transport.pause",
        "Pause",
        "",
        "Pause playback and keep the playhead where it is."
    ),
    (
        "transport.record",
        "Record",
        "Shift+Space",
        "Record from the input device."
    ),
    (
        "transport.loop",
        "Toggle Loop",
        "",
        "Loop the selection (or the whole file) during playback."
    ),
    (
        "transport.play_selection",
        "Play Selection",
        "",
        "Play only the selected range, looping until stopped."
    ),
    (
        "transport.return_play",
        "Return to Start and Play",
        "",
        "Move the playhead to the beginning and play."
    ),
    (
        "transport.to_start",
        "Playhead to Start",
        "Home",
        "Move the playhead to the start."
    ),
    (
        "transport.to_end",
        "Playhead to End",
        "End",
        "Move the playhead to the end."
    ),
    (
        "view.waveform",
        "Waveform Editor",
        "8",
        "Switch to the Waveform editor."
    ),
    (
        "view.multitrack",
        "Multitrack Editor",
        "9",
        "Switch to the Multitrack editor."
    ),
    ("view.mixer", "Mixer", "", "Show the Mixer."),
    (
        "view.spectral",
        "Spectral Frequency Display",
        "Shift+D",
        "Show or hide the spectral display."
    ),
    (
        "view.pitch_display",
        "Spectral Pitch Display",
        "",
        "Show or hide the pitch of the sound over time (Waveform Editor)."
    ),
    (
        "window.workspace_reset",
        "Reset Workspace to Saved Layout",
        "",
        "Put the panels of the current workspace back where they were saved."
    ),
    (
        "window.workspace_save",
        "Save Changes to this Workspace",
        "",
        "Keep the current panel arrangement as this workspace."
    ),
    ("view.zoom_in", "Zoom In", "=", "Zoom in on time."),
    ("view.zoom_out", "Zoom Out", "-", "Zoom out on time."),
    (
        "view.zoom_full",
        "Zoom Out Full",
        "\\",
        "Show the whole file or session."
    ),
    (
        "view.zoom_selection",
        "Zoom to Selection",
        "",
        "Fit the selection in the view."
    ),
    (
        "view.envelopes",
        "Show Clip Volume Envelopes",
        "",
        "Show or hide the yellow volume lines on clips."
    ),
    (
        "view.pan_envelopes",
        "Show Clip Pan Envelopes",
        "",
        "Show or hide the blue pan lines on clips (click to add a point, drag to move, double-click to remove)."
    ),
    (
        "window.clip_effects",
        "Clip Effects",
        "",
        "Show the Effects Rack for the selected clip."
    ),
    (
        "window.track_effects",
        "Track Effects",
        "",
        "Show the Effects Rack for the selected track."
    ),
    ("tool.move", "Move Tool", "V", "Move clips in Multitrack."),
    (
        "tool.razor",
        "Razor Tool",
        "R",
        "Split clips where you click (Shift splits all tracks)."
    ),
    (
        "tool.slip",
        "Slip Tool",
        "Y",
        "Slide the audio inside a clip."
    ),
    (
        "tool.time",
        "Time Selection Tool",
        "T",
        "Select a time range."
    ),
    (
        "tool.marquee",
        "Marquee Selection Tool",
        "E",
        "Spectral display: drag a rectangle of time and frequency; drag inside it to move it, drag its edges to resize it."
    ),
    (
        "tool.lasso",
        "Lasso Selection Tool",
        "D",
        "Spectral display: draw a free-form selection of time and frequency."
    ),
    (
        "tool.brush",
        "Paintbrush Selection Tool",
        "P",
        "Spectral display: paint a selection (Shift adds to it)."
    ),
    (
        "tool.spot_heal",
        "Spot Healing Brush Tool",
        "B",
        "Spectral display: paint over a click or noise to heal it away."
    ),
    (
        "spectral.silence",
        "Silence Spectral Selection",
        "",
        "Remove the sound inside the spectral selection."
    ),
    (
        "spectral.heal",
        "Heal Spectral Selection",
        "",
        "Replace the sound inside the spectral selection with the level around it."
    ),
    (
        "spectral.deselect",
        "Deselect Spectral Selection",
        "",
        "Clear the spectral selection."
    ),
    (
        "spectral.invert",
        "Select Inverse (Spectral)",
        "",
        "Select the other frequencies over the same time range as the spectral selection."
    ),
    (
        "spectral.attenuate",
        "Attenuate Spectral Selection",
        "",
        "Turn the selected frequencies down by 12 dB."
    ),
    (
        "spectral.select_all",
        "Select All Frequencies",
        "",
        "Select the whole time and frequency range of the spectral display."
    ),
    (
        "view.toggle_metronome",
        "Toggle Metronome",
        "",
        "Show or hide the metronome row in Multitrack (its clicks play with the session)."
    ),
    (
        "view.toggle_clip_stretch",
        "Toggle Global Clip Stretching",
        "",
        "Show stretch handles on every clip's top corners; drag them to change a clip's length (the % shows the new length)."
    ),
    (
        "view.toggle_snapping",
        "Toggle Snapping",
        "",
        "Snap clips, drops and the playhead to the chosen targets: clip starts and ends, markers, the ruler's time marks, the playhead and the session start (the magnet's menu in the Multitrack Tools corner picks them)."
    ),
    (
        "view.multitrack_tools",
        "Multitrack Tools",
        "",
        "Show or hide the Multitrack Tools in the multitrack view's corner (header views, metronome, stretching, snapping with its targets, auto-scroll)."
    ),
    (
        "view.toggle_auto_scroll",
        "Toggle Playback Auto-Scroll",
        "",
        "Keep the playhead in view while playing."
    ),
    (
        "view.theme_toggle",
        "Next Colour Theme",
        "",
        "Switch to the next colour theme (AudioCraft, Audition Dark, then the PhotoCraft and VectorCraft themes)."
    ),
    (
        "view.track_header_io",
        "Track Headers: Inputs/Outputs",
        "",
        "Show each track's input and output in its header."
    ),
    (
        "view.track_header_fx",
        "Track Headers: Effects",
        "",
        "Show each track's effects in its header."
    ),
    (
        "view.track_header_sends",
        "Track Headers: Sends",
        "",
        "Show each track's send in its header."
    ),
    (
        "view.track_header_eq",
        "Track Headers: EQ",
        "",
        "Show each track's EQ in its header (the pencil opens the Track EQ)."
    ),
    (
        "view.spectral_zoom_reset",
        "Reset Frequency Zoom",
        "",
        "Show the whole frequency range in the spectral display."
    ),
    (
        "clip.split",
        "Split at Playhead",
        "Ctrl+K",
        "Split the selected clip (or all clips) at the playhead."
    ),
    (
        "clip.split_markers",
        "Split Clips at Markers",
        "",
        "Split every clip that crosses a marker."
    ),
    (
        "clip.duplicate",
        "Duplicate Clip",
        "Ctrl+D",
        "Copy the selected clip to start just after itself."
    ),
    (
        "clip.fade_in",
        "Fade Selected Clip In",
        "",
        "Apply a 0.25 s fade at the start of the selected clip."
    ),
    (
        "clip.fade_out",
        "Fade Selected Clip Out",
        "",
        "Apply a 0.25 s fade at the end of the selected clip."
    ),
    (
        "clip.crossfade",
        "Crossfade with Next Clip",
        "",
        "Turn the overlap with the next clip into a balanced crossfade."
    ),
    (
        "clip.mixdown_new",
        "Mixdown Session to New File",
        "",
        "Render the session into a new file."
    ),
    (
        "track.add_stereo",
        "Add Stereo Track",
        "",
        "Add a stereo track to the session."
    ),
    (
        "track.add_mono",
        "Add Mono Track",
        "",
        "Add a mono track to the session."
    ),
    (
        "track.duplicate",
        "Duplicate Track",
        "Ctrl+Shift+D",
        "Copy the selected track and all its clips."
    ),
    (
        "track.delete",
        "Delete Track",
        "",
        "Remove the selected track and its clips."
    ),
    (
        "track.move_up",
        "Move Track Up",
        "",
        "Move the selected track one place up."
    ),
    (
        "track.move_down",
        "Move Track Down",
        "",
        "Move the selected track one place down."
    ),
    (
        "clip.match_loudness",
        "Match Clip Loudness",
        "",
        "Set the gain of every clip on the selected track so they all play at the same average loudness."
    ),
    (
        "help.settings_folder",
        "Open Settings Folder",
        "",
        "Show the folder where preferences, workspaces and recent files are kept."
    ),
    (
        "track.rename",
        "Rename Track...",
        "",
        "Change the name of the selected track."
    ),
    (
        "track.mute_all",
        "Mute All Tracks",
        "",
        "Mute or unmute every track at once."
    ),
    (
        "track.solo_clear",
        "Clear All Solos",
        "",
        "Turn off solo on every track."
    ),
    (
        "view.volume_automation",
        "Show/Hide Volume Automation Lane",
        "Alt+A",
        "Use the chevron beside the Read selector to open or close the selected track's volume-automation lane."
    ),
    (
        "track.automation_point",
        "Add Volume Automation Point",
        "",
        "Add a volume-automation point at the playhead on the selected track (or Master)."
    ),
    (
        "track.automation_clear",
        "Clear Volume Automation",
        "",
        "Remove every volume-automation point from the selected track (or Master)."
    ),
    (
        "window.essential_sound",
        "Essential Sound",
        "Shift+E",
        "Show or hide the Essential Sound panel."
    ),
    (
        "window.effects_rack",
        "Effects Rack",
        "",
        "Show the Effects Rack."
    ),
    ("window.markers", "Markers", "", "Show the Markers panel."),
    (
        "window.amplitude_statistics",
        "Amplitude Statistics",
        "",
        "Show peak, RMS and loudness numbers."
    ),
    (
        "window.frequency_analysis",
        "Frequency Analysis",
        "",
        "Show the frequency spectrum."
    ),
    (
        "window.command_palette",
        "Command Palette",
        "Ctrl+Shift+P",
        "Search and run any command."
    ),
    (
        "process.invert",
        "Invert",
        "",
        "Flip the polarity of the selection."
    ),
    (
        "process.reverse",
        "Reverse",
        "",
        "Play the selection backwards."
    ),
    (
        "process.silence",
        "Silence",
        "",
        "Replace the selection with silence."
    ),
    ("process.remove_dc", "DC Offset", "", "Remove DC offset."),
    (
        "process.gain",
        "Gain...",
        "Ctrl+G",
        "Raise or lower the selection by a number of dB."
    ),
    (
        "process.declip",
        "Repair Clipped Peaks",
        "",
        "Rebuild flat-topped peaks from the audio around them."
    ),
    (
        "process.normalize",
        "Normalize",
        "",
        "Raise the peak to a target level."
    ),
    ("process.fade_in", "Fade In", "", "Fade in the selection."),
    (
        "process.fade_out",
        "Fade Out",
        "",
        "Fade out the selection."
    ),
    (
        "process.match_loudness",
        "Match Loudness",
        "",
        "Set the loudness to a target in LUFS."
    ),
    (
        "process.stretch_pitch",
        "Stretch and Pitch",
        "",
        "Time-stretch keeping pitch, pitch-shift keeping length, or varispeed."
    ),
    (
        "process.capture_noise_print",
        "Capture Noise Print",
        "",
        "Learn the noise from the selection."
    ),
    (
        "process.noise_reduction",
        "Noise Reduction",
        "",
        "Remove the captured noise."
    ),
    (
        "generate.tones",
        "Generate Tones",
        "",
        "Insert a test tone."
    ),
    (
        "generate.speech",
        "Generate Speech...",
        "",
        "Speak text into a new audio file using an installed voice (Windows SAPI, or espeak/flite on other systems)."
    ),
    (
        "generate.dtmf",
        "Generate DTMF Tones",
        "",
        "Insert telephone dial tones for a dial string."
    ),
    (
        "process.auto_heal",
        "Auto Heal Selection",
        "Ctrl+U",
        "Rebuild a short selection (click, cough) from the audio around it."
    ),
    (
        "process.fade_envelope",
        "Fade Envelope",
        "",
        "Shape the level of the selection with a drawn curve."
    ),
    (
        "process.gain_envelope",
        "Gain Envelope",
        "",
        "Change the gain across the selection with a drawn dB curve."
    ),
    (
        "process.delete_silence",
        "Delete Silence",
        "",
        "Remove quiet passages."
    ),
    (
        "process.mark_audio",
        "Mark Audio",
        "",
        "Add markers where sound starts after a pause."
    ),
    (
        "process.click_pop",
        "Click/Pop Eliminator",
        "",
        "Find and redraw clicks and pops in the selection."
    ),
    (
        "process.phase_correction",
        "Automatic Phase Correction",
        "",
        "Line up the right channel with the left."
    ),
    (
        "process.pitch_bender",
        "Pitch Bender",
        "",
        "Bend pitch and speed from a start to an end value."
    ),
    (
        "window.plugin_manager",
        "Audio Plug-In Manager",
        "",
        "List built-in and installed VST3/CLAP effects and scan for new ones."
    ),
    (
        "window.plugins",
        "Plug-ins",
        "",
        "Show or hide the Plug-ins panel, where VST3 and CLAP effects open their own windows (docked at the right)."
    ),
    (
        "plugins.pop_out",
        "Pop Out / Dock Plug-in Window",
        "",
        "Move the front plug-in editor from the Plug-ins panel into its own floating window, or dock a floating one back."
    ),
    (
        "plugins.close_all",
        "Close All Plug-in Windows",
        "",
        "Close every open VST3/CLAP editor; each plug-in keeps its settings."
    ),
    (
        "plugins.toggle_dock",
        "Dock Plug-in Windows in the Panel",
        "",
        "Open VST3/CLAP editors in the Plug-ins panel, or in their own floating windows."
    ),
    ("effect.phaser", "Phaser", "", "Sweeping all-pass phaser."),
    (
        "effect.chorus_flanger",
        "Chorus/Flanger",
        "",
        "Chorus or flanger in one effect."
    ),
    (
        "effect.analog_delay",
        "Analog Delay",
        "",
        "Delay with a darkening, saturating feedback path."
    ),
    (
        "effect.echo",
        "Echo",
        "",
        "Separate left/right echoes with tone control."
    ),
    (
        "effect.deesser",
        "DeEsser",
        "",
        "Turn down harsh sibilance."
    ),
    (
        "effect.multiband_compressor",
        "Multiband Compressor",
        "",
        "Three-band compressor."
    ),
    (
        "effect.dynamics_processing",
        "Dynamics Processing",
        "",
        "Expander, compressor and limiter in one."
    ),
    (
        "effect.tube_modeled_compressor",
        "Tube-modeled Compressor",
        "",
        "Compressor with warm saturation."
    ),
    (
        "effect.speech_volume_leveler",
        "Speech Volume Leveler",
        "",
        "Even out spoken levels automatically."
    ),
    (
        "effect.channel_mixer",
        "Channel Mixer",
        "",
        "Remix the left and right channels."
    ),
    (
        "effect.graphic_equalizer",
        "Graphic Equalizer",
        "",
        "10, 20 or 30 band graphic EQ."
    ),
    (
        "effect.notch_filter",
        "Notch Filter",
        "",
        "Cut up to six narrow frequencies."
    ),
    (
        "effect.scientific_filter",
        "Scientific Filter",
        "",
        "Butterworth low/high/band pass or band stop."
    ),
    (
        "effect.fft_filter",
        "FFT Filter",
        "",
        "Draw any frequency response (linear phase)."
    ),
    (
        "effect.studio_reverb",
        "Studio Reverb",
        "",
        "Pre-delay, early reflections and a damped tail."
    ),
    (
        "effect.full_reverb",
        "Full Reverb",
        "",
        "Studio reverb with stronger early reflections."
    ),
    (
        "effect.convolution_reverb",
        "Convolution Reverb",
        "",
        "Reverb from impulse responses (rooms, halls, plate)."
    ),
    (
        "effect.guitar_suite",
        "Guitar Suite",
        "",
        "Compressor, distortion, tone and cabinet."
    ),
    (
        "effect.mastering",
        "Mastering",
        "",
        "EQ, exciter, widener and loudness maximizer."
    ),
    (
        "effect.vocal_enhancer",
        "Vocal Enhancer",
        "",
        "Clarity for male, female or music voices."
    ),
    (
        "effect.center_channel_extractor",
        "Center Channel Extractor",
        "",
        "Keep or remove what is in the centre of the stereo image."
    ),
    (
        "effect.graphic_phase_shifter",
        "Graphic Phase Shifter",
        "",
        "Shift the phase of each channel."
    ),
    (
        "effect.adaptive_noise_reduction",
        "Adaptive Noise Reduction",
        "",
        "Removes steady noise it learns while playing."
    ),
    (
        "effect.denoise",
        "DeNoise",
        "",
        "Gentle broadband noise reduction."
    ),
    (
        "effect.hiss_reduction",
        "Hiss Reduction",
        "",
        "Reduce hiss above a frequency."
    ),
    (
        "effect.dereverb",
        "DeReverb",
        "",
        "Reduce room reverberation."
    ),
    (
        "effect.automatic_click_remover",
        "Automatic Click Remover",
        "",
        "Remove clicks and crackle in real time."
    ),
    (
        "generate.noise",
        "Generate Noise",
        "",
        "Insert white, pink or brown noise."
    ),
    ("effect.amplify", "Amplify", "", "Change the level."),
    (
        "effect.parametric_eq",
        "Parametric Equalizer",
        "",
        "Six-band EQ with a draggable curve."
    ),
    (
        "effect.simple_filter",
        "Simple Filter",
        "",
        "Low/high/band pass, notch, peak and shelves."
    ),
    (
        "effect.compressor",
        "Single-band Compressor",
        "",
        "Even out the dynamics."
    ),
    (
        "effect.hard_limiter",
        "Hard Limiter",
        "",
        "Stop peaks above a ceiling."
    ),
    (
        "effect.noise_gate",
        "Noise Gate",
        "",
        "Silence quiet parts."
    ),
    (
        "effect.dehummer",
        "DeHummer",
        "",
        "Remove 50/60 Hz hum and harmonics."
    ),
    ("effect.delay", "Delay", "", "Echo with feedback."),
    ("effect.reverb", "Reverb", "", "Room reverb."),
    (
        "effect.chorus",
        "Chorus",
        "",
        "Thicken with modulated delays."
    ),
    (
        "effect.flanger",
        "Flanger",
        "",
        "Sweeping comb-filter effect."
    ),
    (
        "effect.distortion",
        "Distortion",
        "",
        "Soft-clipping saturation."
    ),
    (
        "effect.stereo_expander",
        "Stereo Expander",
        "",
        "Widen or narrow the stereo image."
    ),
    (
        "favorite.fade_in",
        "Favorite: Fade In",
        "",
        "Cosine fade in."
    ),
    (
        "favorite.fade_out",
        "Favorite: Fade Out",
        "",
        "Cosine fade out."
    ),
    (
        "favorite.normalize_0",
        "Favorite: Normalize to -0.1 dB",
        "",
        "Normalize to -0.1 dB."
    ),
    (
        "favorite.normalize_3",
        "Favorite: Normalize to -3 dB",
        "",
        "Normalize to -3 dB."
    ),
    (
        "favorite.reduce_rumble",
        "Favorite: Reduce Rumble",
        "",
        "High-pass at 80 Hz."
    ),
    (
        "favorite.telephone",
        "Favorite: Telephone Voice",
        "",
        "Band-limited telephone sound."
    ),
    (
        "favorite.podcast_voice",
        "Favorite: Podcast Voice",
        "",
        "EQ, compression and limiting for speech."
    ),
    (
        "favorite.light_reverb",
        "Favorite: Light Reverb",
        "",
        "A touch of room."
    ),
    (
        "help.shortcuts",
        "Keyboard Shortcuts",
        "",
        "List the keyboard shortcuts."
    ),
    ("help.about", "About AudioCraft", "", "Version information."),
];

pub fn find(id: &str) -> Option<&'static Cmd> {
    all().find(|c| c.id == id)
}

/// Every registered command: the core list and the file/edit/clip additions.
pub fn all() -> impl Iterator<Item = &'static Cmd> {
    COMMANDS
        .iter()
        .chain(crate::filecmds::MORE_COMMANDS.iter())
        .chain(crate::extras::EXTRA_COMMANDS.iter())
        .chain(crate::menus_more::MENU_COMMANDS.iter())
}

fn effect_by_id(id: &str) -> Option<Effect> {
    let want = id.trim_start_matches("effect.");
    Effect::all_defaults().into_iter().find(|e| {
        e.name().to_lowercase().replace([' ', '-', '/'], "_") == want || slug(e.name()) == want
    })
}

fn slug(s: &str) -> String {
    let s = s
        .to_lowercase()
        .replace("single-band ", "")
        .replace("parametric equalizer", "parametric_eq");
    s.replace([' ', '-'], "_")
}

/// The clip that starts next after clip `ci` on the same track (by time, not list order).
pub fn next_clip_index(clips: &[crate::session::Clip], ci: usize) -> Option<usize> {
    let cur = clips.get(ci)?;
    clips
        .iter()
        .enumerate()
        .filter(|(j, c)| *j != ci && (c.start > cur.start || (c.start == cur.start && *j > ci)))
        .min_by(|a, b| a.1.start.total_cmp(&b.1.start))
        .map(|(j, _)| j)
}

fn favorite_name(id: &str) -> Option<&'static str> {
    Some(match id {
        "favorite.fade_in" => "Fade In",
        "favorite.fade_out" => "Fade Out",
        "favorite.normalize_0" => "Normalize to -0.1 dB",
        "favorite.normalize_3" => "Normalize to -3 dB",
        "favorite.reduce_rumble" => "Reduce Rumble",
        "favorite.telephone" => "Telephone Voice",
        "favorite.podcast_voice" => "Podcast Voice",
        "favorite.light_reverb" => "Light Reverb",
        _ => return None,
    })
}

impl App {
    /// Run a command by ID. Unknown IDs return an error instead of panicking.
    pub fn exec(&mut self, id: &str, ctx: &egui::Context) -> Result<(), String> {
        match id {
            "file.import_video" => {
                self.import_video_dialog();
                return Ok(());
            }
            "file.import_folder" => {
                if cfg!(test) {
                    return Err("Choose a folder in the dialog.".into());
                }
                if let Some(dir) = rfd::FileDialog::new().ask_folder() {
                    self.open_path(&dir);
                }
                return Ok(());
            }
            "video.detect_decoder" => {
                self.detect_video_decoder();
                return Ok(());
            }
            _ => {}
        }
        let simple = match id {
            "file.new" => Some("new"),
            "file.new_session" => Some("newsession"),
            "file.open" => Some("open"),
            "file.save" => Some("save"),
            "file.save_as" => Some("saveas"),
            "file.close" => Some("close"),
            "edit.undo" => Some("undo"),
            "edit.redo" => Some("redo"),
            "edit.cut" => Some("cut"),
            "edit.copy" => Some("copy"),
            "edit.paste" => Some("paste"),
            "edit.mix_paste" => Some("mixpaste"),
            "edit.paste_new" => Some("pastenew"),
            "edit.delete" => Some("delete"),
            "edit.ripple_delete" => Some("rippledelete"),
            "edit.crop" => Some("crop"),
            "edit.select_all" => Some("all"),
            "edit.deselect" => Some("none"),
            "marker.add" => Some("marker"),
            "transport.play" => Some("play"),
            "transport.record" => Some("record"),
            "transport.to_start" => Some("home"),
            "transport.to_end" => Some("end"),
            "view.waveform" => Some("wave"),
            "view.multitrack" => Some("multi"),
            "view.spectral" => Some("spectral"),
            "view.zoom_in" => Some("zoomin"),
            "view.zoom_out" => Some("zoomout"),
            "view.zoom_full" => Some("zoomfull"),
            "tool.move" => Some("tmove"),
            "tool.razor" => Some("trazor"),
            "tool.slip" => Some("tslip"),
            "tool.time" => Some("tsel"),
            "tool.marquee" => Some("tmarquee"),
            "tool.lasso" => Some("tlasso"),
            "tool.brush" => Some("tbrush"),
            "tool.spot_heal" => Some("theal"),
            "clip.split" => Some("split"),
            "window.essential_sound" => Some("essential"),
            _ => None,
        };
        if let Some(k) = simple {
            self.run(k, ctx);
            return Ok(());
        }
        let need_file =
            id.starts_with("process.") || id.starts_with("effect.") || id.starts_with("favorite.");
        if need_file && (self.mode != Mode::Waveform || self.cur().is_none()) {
            return Err("Open a file in the Waveform editor first.".into());
        }
        if crate::filecmds::repeatable(id) {
            self.last_cmd = Some(id.to_string());
        }
        if let Some(name) = favorite_name(id) {
            self.apply_favorite(name);
            return Ok(());
        }
        if id.starts_with("effect.") {
            let e = effect_by_id(id).ok_or_else(|| format!("Unknown effect '{id}'"))?;
            self.dialog = Some(Dialog::Effect {
                effect: e,
                preview: false,
            });
            return Ok(());
        }
        match id {
            "spectral.silence" => {
                if !self.spectral_apply(
                    crate::spectral::SpecOp::Gain(-200.0),
                    "Silence Spectral Selection",
                ) {
                    return Err(
                        "Make a spectral selection first (Marquee, Lasso or Paintbrush tool)."
                            .into(),
                    );
                }
            }
            "spectral.heal" => {
                if !self.spectral_apply(crate::spectral::SpecOp::Heal, "Heal Spectral Selection") {
                    return Err(
                        "Make a spectral selection first (Marquee, Lasso or Paintbrush tool)."
                            .into(),
                    );
                }
            }
            "view.toggle_metronome" => {
                self.ensure_session();
                if let Some(s) = self.session.as_mut() {
                    s.metronome.on = !s.metronome.on;
                    s.touch();
                }
            }
            "view.toggle_clip_stretch" => self.stretch_mode = !self.stretch_mode,
            "view.pitch_display" => self.pitch_display = !self.pitch_display,
            "window.workspace_reset" => self.ws.reset(),
            "window.workspace_save" => self.ws.save_current(),
            "view.toggle_snapping" => self.snapping = !self.snapping,
            "view.multitrack_tools" => self.show_tools_window = !self.show_tools_window,
            "view.toggle_auto_scroll" => self.auto_scroll = !self.auto_scroll,
            "view.theme_toggle" => {
                // the next theme in the list, round to the first
                let kind =
                    ((self.prefs.color_theme as usize + 1) % crate::theme::theme_count()) as u8;
                self.prefs.color_theme = kind;
                crate::theme::set_theme(kind);
                if let Some(ctx) = self.engine.ctx.as_ref() {
                    crate::theme::setup(ctx);
                }
                crate::prefs::save(&self.prefs);
            }
            "view.track_header_io" => self.header_view = 0,
            "view.track_header_fx" => self.header_view = 1,
            "view.track_header_sends" => self.header_view = 2,
            "view.track_header_eq" => self.header_view = 3,
            "spectral.invert" => {
                let f = self.cur_mut().ok_or("No file open")?;
                let top = f.data.rate as f32 / 2.0;
                let s = f.spec_sel.take().ok_or(
                    "Make a spectral selection first (Marquee, Lasso or Paintbrush tool).",
                )?;
                f.spec_sel = Some(s.inverted(top));
            }
            "spectral.deselect" => {
                if let Some(f) = self.cur_mut() {
                    f.spec_sel = None;
                }
            }
            "view.spectral_zoom_reset" => {
                self.drag_state.spec_lo = 0.0;
                self.drag_state.spec_hi = 0.0;
            }
            "file.export_mixdown" => {
                self.open_export(crate::export::ExportSource::Mixdown { selection: false })?
            }
            "edit.insert_silence" => self.dialog = Some(Dialog::InsertSilence { secs: 1.0 }),
            "edit.convert" => {
                let f = self.cur().ok_or("No file open")?;
                self.dialog = Some(Dialog::Convert {
                    rate: f.data.rate,
                    channels: f.data.channels(),
                    bits: if f.float { 32 } else { f.bits },
                });
            }
            "edit.preferences" => self.open_prefs("General"),
            "transport.pause" => self.pause(),
            "transport.loop" => {
                let mut s = self.engine.lock();
                s.looping = !s.looping;
            }
            "view.mixer" => {
                self.ensure_mixer_session();
                self.switch_mode(Mode::Multitrack);
                self.center_tab = crate::app::CenterTab::Mixer;
            }
            "view.zoom_selection" => self.zoom_selection(),
            "view.envelopes" => self.show_env = !self.show_env,
            "view.pan_envelopes" => self.show_pan_env = !self.show_pan_env,
            "tool.direct_selection" => self.tool = crate::app::Tool::Direct,
            "keyframes.delete" => {
                if !self.delete_selected_keys() {
                    return Err("No keyframes are selected".into());
                }
            }
            "keyframes.deselect" => self.multi_drag.key_sel.clear(),
            "session.convert_for_audition" => {
                let Some(s) = self.session.as_mut() else {
                    return Err("No session is open".into());
                };
                let n = crate::compat::convert(s, self.prefs.sesx_bake_tolerance_db);
                s.touch();
                self.session_push("Convert for Audition");
                self.status = format!("Converted for Audition ({n} keyframes)");
            }
            "view.spectral_log" => {
                self.prefs.spec_log_scale = !self.prefs.spec_log_scale;
                crate::prefs::save(&self.prefs);
            }
            "view.spectral_pitch" => self.pitch_display = !self.pitch_display,
            "files.play" => {
                if self.files_preview.is_some() {
                    self.stop_file_preview();
                } else if let Some(id) = self.selected_file.or(self.current) {
                    self.start_file_preview(id);
                } else {
                    return Err("No file is selected".into());
                }
            }
            "files.loop" => {
                self.files_loop = !self.files_loop;
                if let Some(p) = self.engine.lock().files_preview.as_mut() {
                    p.looping = self.files_loop;
                }
            }
            "files.autoplay" => self.files_autoplay = !self.files_autoplay,

            "window.clip_effects" | "window.track_effects" => {
                self.switch_mode(Mode::Multitrack);
                self.show_panel(crate::workspace::PanelId::EffectsRack);
                self.rack_clip_tab = id == "window.clip_effects";
            }
            "clip.mixdown_new" => self.mixdown(false, false),
            "track.add_stereo" | "track.add_mono" => {
                self.ensure_session();
                if let Some(s) = self.session.as_mut() {
                    s.add_track(id == "track.add_stereo");
                }
                self.session_push("Add Track");
                self.switch_mode(Mode::Multitrack);
            }
            "window.effects_rack" | "window.markers" => {
                self.show_panel(if id == "window.markers" {
                    crate::workspace::PanelId::Markers
                } else {
                    crate::workspace::PanelId::EffectsRack
                });
            }
            "window.amplitude_statistics" => self.show_panel(crate::workspace::PanelId::AmpStats),
            "window.frequency_analysis" => self.show_panel(crate::workspace::PanelId::FreqAnalysis),
            "window.command_palette" => self.palette = Some(String::new()),
            "process.invert" => self.process("Invert", dsp::invert),
            "process.reverse" => self.process("Reverse", dsp::reverse),
            "process.silence" => self.process("Silence", dsp::silence),
            "process.remove_dc" => self.process("DC Offset", dsp::remove_dc),
            "process.normalize" => self.dialog = Some(Dialog::Normalize { db: -0.1 }),
            "process.fade_in" => {
                self.dialog = Some(Dialog::Fade {
                    fade_in: true,
                    curve: FadeCurve::Linear,
                })
            }
            "process.fade_out" => {
                self.dialog = Some(Dialog::Fade {
                    fade_in: false,
                    curve: FadeCurve::Linear,
                })
            }
            "process.match_loudness" => self.dialog = Some(Dialog::Loudness { target: -23.0 }),
            "process.stretch_pitch" => {
                self.dialog = Some(Dialog::Speed {
                    ratio: 1.0,
                    semis: 0.0,
                    mode: 1,
                })
            }
            "process.capture_noise_print" => {
                let f = self.cur().ok_or("No file open")?;
                let (a, b) = f.range();
                self.noise_profile = Some(dsp::capture_noise(&f.data, a, b));
                self.status = "Noise print captured".into();
            }
            "process.noise_reduction" => {
                self.dialog = Some(Dialog::NoiseReduction {
                    amount: 0.8,
                    reduce_db: 20.0,
                })
            }
            "generate.tones" => {
                self.dialog = Some(Dialog::Tone {
                    freq: 440.0,
                    wave: Wave::Sine,
                    db: -6.0,
                    secs: 5.0,
                })
            }
            "generate.speech" => {
                self.dialog = Some(Dialog::Speech {
                    pitch: 0.0,
                    text: String::new(),
                    voice: String::new(),
                    rate: 0,
                    tab: 0,
                    tract: Default::default(),
                })
            }
            "generate.noise" => {
                self.dialog = Some(Dialog::Noise {
                    color: NoiseColor::Pink,
                    db: -12.0,
                    secs: 5.0,
                })
            }
            "generate.dtmf" => {
                self.dialog = Some(Dialog::Dtmf {
                    digits: "0123456789".into(),
                    tone_ms: 120.0,
                    pause_ms: 80.0,
                    db: -6.0,
                })
            }
            "process.auto_heal" => self.auto_heal(),
            "process.fade_envelope" => {
                self.dialog = Some(Dialog::Envelope {
                    gain: false,
                    points: vec![(0.0, 1.0), (1.0, 1.0)],
                })
            }
            "process.gain_envelope" => {
                self.dialog = Some(Dialog::Envelope {
                    gain: true,
                    points: vec![(0.0, 0.0), (1.0, 0.0)],
                })
            }
            "process.delete_silence" => {
                self.dialog = Some(Dialog::DeleteSilence {
                    thr_db: -50.0,
                    min_s: 0.5,
                    keep_s: 0.2,
                })
            }
            "process.mark_audio" => {
                self.dialog = Some(Dialog::MarkAudio {
                    thr_db: -50.0,
                    min_s: 0.5,
                })
            }
            "process.click_pop" => self.dialog = Some(Dialog::ClickPop { sensitivity: 0.5 }),
            "process.phase_correction" => {
                self.process("Automatic Phase Correction", |d, a, b| {
                    crate::procs::phase_correct(d, a, b, 5.0);
                });
            }
            "process.pitch_bender" => {
                self.dialog = Some(Dialog::PitchBender {
                    start_st: 0.0,
                    end_st: 2.0,
                })
            }
            "window.plugin_manager" => self.dialog = Some(Dialog::PluginManager),
            "window.plugins" => {
                self.ws.layout.toggle(crate::workspace::PanelId::PlugIns);
                if !self.ws.layout.contains(crate::workspace::PanelId::PlugIns)
                    && !self.plug_dock.wins.is_empty()
                {
                    self.status = "Plug-ins panel hidden; its plug-in windows stay open (Window > Plug-ins shows them again)".into();
                }
            }
            "plugins.close_all" => self.plug_close_all(),
            "plugins.pop_out" => self.plug_toggle_pop()?,
            "plugins.toggle_dock" => {
                self.prefs.dock_plugin_editors = !self.prefs.dock_plugin_editors;
                crate::prefs::save(&self.prefs);
                self.status = if self.prefs.dock_plugin_editors {
                    "Plug-in windows now open in the Plug-ins panel (reopen an open one to move it)"
                        .into()
                } else {
                    "Plug-in windows now open in their own windows (reopen an open one to move it)"
                        .into()
                };
            }
            "help.shortcuts" => self.dialog = Some(Dialog::Shortcuts),
            "help.about" => self.dialog = Some(Dialog::About),

            // ---- marker management and navigation -----------------------------------------
            "marker.next" | "marker.previous" => {
                let t = self.cursor();
                let ms = self.marker_snapshot();
                let found = if id == "marker.next" {
                    crate::workflows::next_marker(&ms, t)
                } else {
                    crate::workflows::previous_marker(&ms, t)
                };
                let Some((_, m)) = found else {
                    return Err("Add a marker first (M).".into());
                };
                let (at, name) = (m.time, m.name.clone());
                self.set_cursor(at);
                self.status = format!("At marker '{name}' ({})", self.fmt(at));
            }
            "marker.rename" => {
                let t = self.cursor();
                let ms = self.marker_snapshot();
                let Some((i, m)) = crate::workflows::nearest_marker(&ms, t) else {
                    return Err("Add a marker first (M).".into());
                };
                self.dialog = Some(Dialog::RenameMarker {
                    index: i,
                    name: m.name.clone(),
                });
            }
            "marker.delete" => {
                let t = self.cursor();
                let ms = self.marker_snapshot();
                let Some((i, _)) = crate::workflows::nearest_marker(&ms, t) else {
                    return Err("Add a marker first (M).".into());
                };
                if self.mode == Mode::Multitrack {
                    if let Some(s) = self.session.as_mut() {
                        if i < s.markers.len() {
                            s.markers.remove(i);
                            s.touch();
                        }
                    }
                    self.session_push("Delete Marker");
                } else if let Some(f) = self.cur_mut() {
                    if i < f.markers.len() {
                        f.markers.remove(i);
                    }
                }
                self.status = "Marker deleted".into();
            }
            "marker.export" => {
                let ms = self.marker_snapshot();
                if ms.is_empty() {
                    return Err("There are no markers to export.".into());
                }
                let Some(path) = rfd::FileDialog::new()
                    .set_file_name("markers.csv")
                    .ask_save()
                else {
                    return Ok(());
                };
                std::fs::write(&path, crate::workflows::markers_to_csv(&ms))
                    .map_err(|e| format!("Could not write markers: {e}"))?;
                self.status = format!("Exported {} markers to {}", ms.len(), path.display());
            }
            "marker.import" => {
                if self.mode == Mode::Waveform && self.cur().is_none() {
                    return Err("Open a file in the Waveform editor first.".into());
                }
                if self.mode == Mode::Multitrack {
                    self.ensure_session();
                }
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("Marker CSV", &["csv"])
                    .ask_file()
                else {
                    return Ok(());
                };
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Could not read markers: {e}"))?;
                let added = crate::workflows::markers_from_csv(&text);
                if added.is_empty() {
                    return Err(
                        "No markers found in that CSV (expected Name, Start, Duration columns)."
                            .into(),
                    );
                }
                let n = added.len();
                if self.mode == Mode::Multitrack {
                    if let Some(s) = self.session.as_mut() {
                        s.markers.extend(added);
                        s.markers.sort_by(|a, b| a.time.total_cmp(&b.time));
                        s.touch();
                    }
                    self.session_push("Import Markers");
                } else if let Some(f) = self.cur_mut() {
                    f.markers.extend(added);
                    f.markers.sort_by(|a, b| a.time.total_cmp(&b.time));
                }
                self.status = format!("Imported {n} markers from {}", path.display());
            }

            // ---- transport -------------------------------------------------------------------
            "transport.play_selection" => {
                let sel = match self.mode {
                    Mode::Waveform => self.cur().and_then(|f| f.selection),
                    Mode::Multitrack => self.session.as_ref().and_then(|s| s.selection),
                };
                let Some((a, b)) = sel.filter(|(a, b)| b - a > 0.001) else {
                    return Err("Make a time selection first, then choose Play Selection.".into());
                };
                self.stop();
                self.set_cursor(a);
                {
                    let mut e = self.engine.lock();
                    e.looping = true;
                    e.loop_start = a;
                }
                self.play();
                self.status = format!("Playing selection {} - {}", self.fmt(a), self.fmt(b));
            }
            "transport.return_play" => {
                self.stop();
                self.set_cursor(0.0);
                self.play();
            }

            // ---- repair and levels -----------------------------------------------------------
            "process.gain" => self.dialog = Some(Dialog::Gain { db: 0.0 }),
            "process.declip" => {
                let mut repaired = 0usize;
                self.process("Repair Clipped Peaks", |d, a, b| {
                    repaired = crate::workflows::declip(d, a, b, 0.999);
                });
                self.status = if repaired > 0 {
                    format!("Rebuilt {repaired} clipped samples")
                } else {
                    "No flat-topped peaks found in the selection".into()
                };
            }

            // ---- multitrack: clips and tracks ------------------------------------------------
            "clip.split_markers" => {
                self.ensure_session();
                let marks: Vec<f64> = self
                    .session
                    .as_ref()
                    .map(|s| s.markers.iter().map(|m| m.time).collect())
                    .unwrap_or_default();
                if marks.is_empty() {
                    return Err("Add markers first: split points come from the marker list.".into());
                }
                let ids: Vec<u64> = self
                    .session
                    .as_ref()
                    .map(|s| {
                        s.tracks
                            .iter()
                            .flat_map(|t| t.clips.iter().map(|c| c.id))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut cuts = 0usize;
                if let Some(s) = self.session.as_mut() {
                    for t in &marks {
                        for cid in &ids {
                            if s.split_clip(*cid, *t).is_some() {
                                cuts += 1;
                            }
                        }
                    }
                }
                self.session_push("Split Clips at Markers");
                self.status = format!("Made {cuts} new clips at {} markers", marks.len());
            }
            "clip.duplicate" => {
                self.ensure_session();
                let spec = self.session.as_ref().and_then(|s| {
                    let (ti, ci) = s.selected_clip.and_then(|id| s.find_clip(id))?;
                    Some((ti, ci, s.tracks[ti].clips[ci].clone()))
                });
                let Some((ti, ci, src)) = spec else {
                    return Err("Select a clip to duplicate.".into());
                };
                let mut new_id;
                {
                    let s = self.session.as_mut().ok_or("No session")?;
                    new_id = s.id();
                }
                let mut copy = crate::workflows::duplicate_clip(&src, new_id, src.end());
                copy.name = format!("{} copy", src.name);
                new_id = copy.id;
                if let Some(s) = self.session.as_mut() {
                    s.tracks[ti].clips.insert(ci + 1, copy);
                    s.selected_clip = Some(new_id);
                    s.touch();
                }
                self.session_push("Duplicate Clip");
                self.status = "Clip duplicated".into();
            }
            "clip.fade_in" | "clip.fade_out" => {
                self.ensure_session();
                let mut applied = false;
                if let Some(s) = self.session.as_mut() {
                    if let Some(c) = s.selected_clip.and_then(|id| s.clip_mut(id)) {
                        crate::workflows::fade_clip(c, id == "clip.fade_in", 0.25);
                        applied = true;
                        s.touch();
                    }
                }
                if !applied {
                    return Err("Select a clip to fade.".into());
                }
                self.session_push(if id == "clip.fade_in" {
                    "Fade Clip In"
                } else {
                    "Fade Clip Out"
                });
            }
            "clip.crossfade" => {
                self.ensure_session();
                let spec = self.session.as_ref().and_then(|s| {
                    let (ti, ci) = s.selected_clip.and_then(|id| s.find_clip(id))?;
                    // the next clip in time (the track's list is not kept in time order)
                    let j = next_clip_index(&s.tracks[ti].clips, ci)?;
                    Some((ti, ci, j))
                });
                let Some((ti, ci, j)) = spec else {
                    return Err(
                        "Select a clip that has another clip after it on the same track.".into(),
                    );
                };
                let overlap = {
                    let s = self.session.as_mut().ok_or("No session")?;
                    let clips = &mut s.tracks[ti].clips;
                    if j > ci {
                        let (head, tail) = clips.split_at_mut(j);
                        crate::workflows::crossfade_pair(&mut head[ci], &mut tail[0], 2.0)
                    } else {
                        let (head, tail) = clips.split_at_mut(ci);
                        crate::workflows::crossfade_pair(&mut tail[0], &mut head[j], 2.0)
                    }
                };
                if overlap.is_some() {
                    if let Some(s) = self.session.as_mut() {
                        s.touch();
                    }
                }
                let Some(secs) = overlap else {
                    return Err("The clips do not overlap (drag them together first).".into());
                };
                self.session_push("Crossfade Clips");
                self.status = format!("Crossfade created over {secs:.2} s");
            }
            "track.duplicate" => {
                self.ensure_session();
                let spec = self
                    .session
                    .as_ref()
                    .and_then(|s| s.tracks.get(s.selected_track).cloned());
                let Some(src) = spec else {
                    return Err("Select a track to duplicate.".into());
                };
                let (tid, mut nid) = {
                    let s = self.session.as_mut().ok_or("No session")?;
                    (s.id(), s.next_id + 1)
                };
                let name = format!("{} copy", src.name);
                let copy = crate::workflows::duplicate_track(&src, name, tid, &mut nid);
                if let Some(s) = self.session.as_mut() {
                    s.next_id = nid.max(s.next_id);
                    s.tracks.insert(s.selected_track + 1, copy);
                    s.selected_track += 1;
                    s.touch();
                }
                self.session_push("Duplicate Track");
            }
            "track.move_up" | "track.move_down" => {
                let up = id == "track.move_up";
                let s = self
                    .session
                    .as_mut()
                    .ok_or("Open a multitrack session first.")?;
                let n = s.tracks.len();
                let i = s.selected_track.min(n.saturating_sub(1));
                let j = if up {
                    i.checked_sub(1)
                } else {
                    (i + 1 < n).then_some(i + 1)
                };
                let Some(j) = j else {
                    return Err(format!(
                        "The track is already at the {}.",
                        if up { "top" } else { "bottom" }
                    ));
                };
                s.tracks.swap(i, j);
                s.selected_track = j;
                s.touch();
                self.session_push("Move Track");
            }
            "clip.match_loudness" => {
                let files = &self.files;
                let s = self
                    .session
                    .as_mut()
                    .ok_or("Open a multitrack session first.")?;
                let n = crate::session::match_clip_loudness(s, |fid| {
                    files.iter().find(|f| f.id == fid).map(|f| f.data.clone())
                });
                if n == 0 {
                    return Err("The selected track has no clips with sound to match.".into());
                }
                s.touch();
                self.session_push("Match Clip Loudness");
                self.status = format!("{n} clips set to the same loudness");
            }
            "help.settings_folder" => {
                let dir = crate::prefs::config_dir().ok_or("No settings folder on this system")?;
                let _ = std::fs::create_dir_all(&dir);
                crate::sysui::open_external(&dir)
                    .map_err(|e| format!("Could not open {}: {e}", dir.display()))?;
            }
            "track.delete" => {
                self.ensure_session();
                let mut removed = false;
                if let Some(s) = self.session.as_mut() {
                    if !s.tracks.is_empty() {
                        let i = s.selected_track.min(s.tracks.len() - 1);
                        s.tracks.remove(i);
                        s.selected_track = i.min(s.tracks.len().saturating_sub(1));
                        s.selected_clip = None;
                        s.touch();
                        removed = true;
                    }
                }
                if !removed {
                    return Err("The session has no tracks to delete.".into());
                }
                self.session_push("Delete Track");
            }
            "track.rename" => {
                self.ensure_session();
                let cur = self.session.as_ref().and_then(|s| {
                    s.tracks
                        .get(s.selected_track)
                        .map(|t| (s.selected_track, t.name.clone()))
                });
                let Some((index, name)) = cur else {
                    return Err("Select a track first.".into());
                };
                self.dialog = Some(Dialog::RenameTrack { index, name });
            }
            "track.mute_all" => {
                self.ensure_session();
                let any_on = self
                    .session
                    .as_ref()
                    .map(|s| s.tracks.iter().any(|t| !t.mute))
                    .unwrap_or(false);
                if let Some(s) = self.session.as_mut() {
                    for t in s.tracks.iter_mut() {
                        t.mute = any_on;
                    }
                    s.touch();
                }
                self.session_push(if any_on {
                    "Mute All Tracks"
                } else {
                    "Unmute All Tracks"
                });
            }
            "track.solo_clear" => {
                self.ensure_session();
                if let Some(s) = self.session.as_mut() {
                    for t in s.tracks.iter_mut() {
                        t.solo = false;
                    }
                    s.touch();
                }
                self.session_push("Clear All Solos");
            }

            // ---- volume automation (the chevron beside each Read selector) -------------------
            "view.volume_automation" => {
                self.ensure_mixer_session();
                self.switch_mode(Mode::Multitrack);
                if let Some(s) = self.session.as_mut() {
                    let master = s.master_selected || s.tracks.is_empty();
                    if master {
                        s.master.show_vol_env = !s.master.show_vol_env;
                    } else if let Some(t) = s.tracks.get_mut(s.selected_track) {
                        t.show_vol_env = !t.show_vol_env;
                        if t.show_vol_env {
                            t.height = t.height.max(120.0);
                            t.minimized = false;
                        }
                    }
                    s.touch();
                }
                self.session_push("Toggle Volume Automation Lane");
            }
            "track.automation_point" => {
                self.ensure_mixer_session();
                self.switch_mode(Mode::Multitrack);
                let t = self.session.as_ref().map(|s| s.cursor).unwrap_or(0.0);
                if let Some(s) = self.session.as_mut() {
                    if s.master_selected || s.tracks.is_empty() {
                        let db = s.master_db;
                        crate::workflows::insert_auto_point(&mut s.master.vol_env, t, db);
                    } else if let Some(track) = s.tracks.get_mut(s.selected_track) {
                        let db = track.volume_db;
                        crate::workflows::insert_auto_point(&mut track.vol_env, t, db);
                        track.show_vol_env = true;
                    }
                    s.touch();
                }
                self.session_push("Add Volume Automation Point");
            }
            "track.automation_clear" => {
                self.ensure_mixer_session();
                if let Some(s) = self.session.as_mut() {
                    if s.master_selected || s.tracks.is_empty() {
                        s.master.vol_env.clear();
                    } else if let Some(track) = s.tracks.get_mut(s.selected_track) {
                        track.vol_env.clear();
                    }
                    s.touch();
                }
                self.session_push("Clear Volume Automation");
            }

            // ---- batch work and analysis ------------------------------------------------------
            "file.batch_process" => {
                // start with the files that are open, into a Batch folder beside the sessions
                let inputs: Vec<String> = self
                    .files
                    .iter()
                    .filter_map(|f| f.path.as_ref())
                    .filter(|p| crate::audio::is_audio_path(p))
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                let out_dir = std::path::Path::new(&crate::dialogs::default_session_folder())
                    .join("Batch")
                    .to_string_lossy()
                    .to_string();
                self.dialog = Some(Dialog::Batch {
                    inputs,
                    preset: 0,
                    out_dir,
                    suffix: "_processed".into(),
                    result: String::new(),
                });
            }
            "file.export_report" => {
                let Some(f) = self.cur() else {
                    return Err("Open a file in the Waveform editor first.".into());
                };
                let (a, b) = f.range();
                let r = crate::workflows::analyze(&f.data, a, b);
                let csv = crate::workflows::report_csv(
                    &f.name,
                    r,
                    b.saturating_sub(a) as f64 / f.data.rate as f64,
                );
                let Some(path) = rfd::FileDialog::new()
                    .set_file_name("analysis_report.csv")
                    .ask_save()
                else {
                    return Ok(());
                };
                std::fs::write(&path, csv).map_err(|e| format!("Could not write report: {e}"))?;
                self.status = format!("Analysis report saved to {}", path.display());
            }

            // ---- spectral ---------------------------------------------------------------------
            "spectral.attenuate" => {
                if !self.spectral_apply(
                    crate::spectral::SpecOp::Gain(-12.0),
                    "Attenuate Spectral Selection",
                ) {
                    return Err(
                        "Make a spectral selection first (Marquee, Lasso or Paintbrush tool)."
                            .into(),
                    );
                }
            }
            "spectral.select_all" => {
                let f = self
                    .cur_mut()
                    .ok_or("Open a file in the Waveform editor first.")?;
                let top = f.data.rate as f32 / 2.0;
                let dur = f.data.duration();
                f.spec_sel = Some(crate::spectral::SpecShape::Rect {
                    t0: 0.0,
                    t1: dur,
                    f0: 0.0,
                    f1: top,
                });
                self.spectral = true;
                self.status = "Selected the whole spectral display".into();
            }
            _ => {
                if !self.exec_more(id, ctx)?
                    && !self.exec_extra(id, ctx)?
                    && !self.exec_menus(id)?
                {
                    return Err(format!("Unknown command '{id}'"));
                }
            }
        }
        Ok(())
    }

    /// A copy of the marker list of the editor that is showing, for navigation and CSV export.
    pub fn marker_snapshot(&self) -> Vec<crate::session::Marker> {
        match self.mode {
            Mode::Multitrack => self
                .session
                .as_ref()
                .map(|s| s.markers.clone())
                .unwrap_or_default(),
            Mode::Waveform => self.cur().map(|f| f.markers.clone()).unwrap_or_default(),
        }
    }

    /// The command palette window (Ctrl+Shift+P).
    pub fn command_palette(&mut self, ctx: &egui::Context) {
        let Some(mut query) = self.palette.take() else {
            return;
        };
        let mut open = true;
        let mut run: Option<&'static str> = None;
        egui::Window::new("Command Palette")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .fixed_size([520.0, 380.0])
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text("Type a command name or ID...")
                        .desired_width(f32::INFINITY),
                );
                r.request_focus();
                let q = query.to_lowercase();
                let matches: Vec<&'static Cmd> = all()
                    .filter(|c| {
                        q.is_empty()
                            || c.title.to_lowercase().contains(&q)
                            || c.id.contains(&q)
                            || c.doc.to_lowercase().contains(&q)
                    })
                    .collect();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    run = matches.first().map(|c| c.id);
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for c in matches {
                            let resp = ui.add(
                                egui::Button::new(egui::RichText::new(c.title).strong())
                                    .right_text(
                                        egui::RichText::new(c.shortcut)
                                            .color(crate::theme::time_blue()),
                                    )
                                    .min_size(egui::vec2(ui.available_width(), 22.0)),
                            );
                            let resp = resp.on_hover_text(format!("{}\n{}", c.id, c.doc));
                            if resp.clicked() {
                                run = Some(c.id);
                            }
                        }
                    });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        if let Some(id) = run {
            if let Err(e) = self.exec(id, ctx) {
                self.message("Cannot run command", &e);
            }
            return;
        }
        if open {
            self.palette = Some(query);
        }
    }
}

// ---------------------------------------------------------------- command line

/// Print the command registry as JSON.
pub fn list_json() -> String {
    let v: Vec<serde_json::Value> = all()
        .map(|c| serde_json::json!({ "id": c.id, "title": c.title, "shortcut": c.shortcut, "doc": c.doc }))
        .collect();
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Ready-made processing chains for the Batch Processor dialog.
pub const BATCH_PRESETS: &[(&str, &[&str])] = &[
    (
        "Podcast voice (normalize to -1 dB)",
        &["favorite.podcast_voice", "process.normalize=-1"],
    ),
    ("Normalize to -1 dB", &["process.normalize=-1"]),
    (
        "Dialogue cleanup",
        &[
            "process.remove_dc",
            "favorite.reduce_rumble",
            "favorite.podcast_voice",
        ],
    ),
    (
        "Warm vocal",
        &[
            "effect.tube_modeled_compressor",
            "favorite.light_reverb",
            "process.normalize=-1",
        ],
    ),
    (
        "Delivery: 44.1 kHz, -1 dB",
        &["process.convert_rate=44100", "process.normalize=-1"],
    ),
];

pub fn batch_preset_steps(index: usize) -> Vec<String> {
    BATCH_PRESETS
        .get(index)
        .map(|(_, steps)| steps.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

/// Steps the headless batch mode understands. Format: `id` or `id=value`.
pub const BATCH_STEPS: &[(&str, &str)] = &[
    ("process.normalize[=dB]", "Normalize peak (default -0.1 dB)"),
    ("process.gain=dB", "Change level"),
    ("process.fade_in=seconds", "Cosine fade in at the start"),
    ("process.fade_out=seconds", "Cosine fade out at the end"),
    ("process.invert", "Invert polarity"),
    ("process.reverse", "Reverse"),
    ("process.remove_dc", "Remove DC offset"),
    (
        "process.match_loudness[=LUFS]",
        "Match loudness (default -23)",
    ),
    (
        "process.stretch=percent",
        "Time-stretch to percent of length, keeping pitch",
    ),
    ("process.pitch=semitones", "Pitch shift keeping length"),
    ("process.convert_rate=Hz", "Resample"),
    ("favorite.*", "Any favorite, e.g. favorite.podcast_voice"),
    (
        "effect.<id>",
        "Any effect with default settings, e.g. effect.reverb",
    ),
];

pub(crate) fn apply_step(d: &mut AudioData, step: &str) -> Result<(), String> {
    let (id, val) = match step.split_once('=') {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (step.trim(), None),
    };
    let num = |def: f64| -> Result<f64, String> {
        match val {
            Some(v) => v
                .parse::<f64>()
                .map_err(|_| format!("'{v}' is not a number for {id}")),
            None => Ok(def),
        }
    };
    let n = d.frames();
    let rate = d.rate as f64;
    match id {
        "process.normalize" => dsp::normalize(d, 0, n, num(-0.1)? as f32),
        "process.gain" => dsp::apply_chain(
            d,
            0,
            n,
            &[EffectSlot::new(Effect::Amplify {
                gain_db: num(0.0)? as f32,
            })],
        ),
        "process.fade_in" => {
            let k = ((num(1.0)? * rate) as usize).min(n);
            dsp::fade(d, 0, k, true, FadeCurve::Cosine)
        }
        "process.fade_out" => {
            let k = ((num(1.0)? * rate) as usize).min(n);
            dsp::fade(d, n - k, n, false, FadeCurve::Cosine)
        }
        "process.invert" => dsp::invert(d, 0, n),
        "process.reverse" => dsp::reverse(d, 0, n),
        "process.remove_dc" => dsp::remove_dc(d, 0, n),
        "process.match_loudness" => dsp::match_loudness(d, 0, n, num(-23.0)? as f32),
        "process.stretch" => {
            let pct = num(100.0)?;
            if pct <= 0.0 {
                return Err("stretch percent must be positive".into());
            }
            *d = dsp::stretch_range(d, 0, n, pct / 100.0);
        }
        "process.pitch" => *d = dsp::pitch_range(d, 0, n, num(0.0)?),
        "process.convert_rate" => {
            let r = num(48000.0)?;
            if !(1000.0..=768000.0).contains(&r) {
                return Err("sample rate out of range".into());
            }
            *d = audio::resample(d, r as u32);
        }
        "favorite.fade_in" => dsp::fade(d, 0, n, true, FadeCurve::Cosine),
        "favorite.fade_out" => dsp::fade(d, 0, n, false, FadeCurve::Cosine),
        "favorite.normalize_0" => dsp::normalize(d, 0, n, -0.1),
        "favorite.normalize_3" => dsp::normalize(d, 0, n, -3.0),
        "favorite.reduce_rumble" => dsp::apply_chain(
            d,
            0,
            n,
            &[EffectSlot::new(Effect::Filter {
                kind: FilterKind::HighPass,
                freq: 80.0,
                q: 0.707,
                gain_db: 0.0,
            })],
        ),
        "favorite.telephone" => dsp::apply_chain(
            d,
            0,
            n,
            &[EffectSlot::new(Effect::ParametricEq {
                bands: crate::panels::eq_preset(3).unwrap_or_default(),
                master_db: 0.0,
            })],
        ),
        "favorite.podcast_voice" => dsp::apply_chain(
            d,
            0,
            n,
            &[
                EffectSlot::new(Effect::ParametricEq {
                    bands: crate::panels::eq_preset(0).unwrap_or_default(),
                    master_db: 0.0,
                }),
                EffectSlot::new(Effect::Compressor {
                    threshold: -22.0,
                    ratio: 3.0,
                    attack_ms: 5.0,
                    release_ms: 120.0,
                    makeup: 4.0,
                }),
                EffectSlot::new(Effect::HardLimiter {
                    ceiling_db: -1.0,
                    input_boost: 0.0,
                    release_ms: 50.0,
                }),
            ],
        ),
        "favorite.light_reverb" => dsp::apply_chain(
            d,
            0,
            n,
            &[EffectSlot::new(Effect::Reverb {
                room: 0.45,
                damping: 0.5,
                width: 1.0,
                mix: 0.18,
            })],
        ),
        other if other.starts_with("effect.") => {
            let e = effect_by_id(other).ok_or_else(|| format!("Unknown effect '{other}'"))?;
            dsp::apply_chain(d, 0, n, &[EffectSlot::new(e)]);
        }
        other => return Err(format!("Unknown batch step '{other}'")),
    }
    Ok(())
}

/// Headless processing: `audiocraft --batch in.wav --do step [--do step...] --out out.wav`.
pub fn run_batch(
    input: &std::path::Path,
    steps: &[String],
    output: &std::path::Path,
) -> Result<String, String> {
    let lf = audio::load(input)?;
    let mut d = lf.data;
    for s in steps {
        apply_step(&mut d, s)?;
    }
    if d.ch.iter().flat_map(|c| c.iter()).any(|v| !v.is_finite()) {
        return Err("processing produced invalid samples".into());
    }
    audio::save_wav(
        output,
        &d,
        if lf.float { 32 } else { lf.bits.max(16) },
        lf.float,
    )?;
    Ok(format!(
        "{} steps applied, wrote {} ({:.3} s, {} Hz, {} ch)",
        steps.len(),
        output.display(),
        d.duration(),
        d.rate,
        d.channels()
    ))
}

/// Batch Processor: run the same steps over a list of files into an output folder.
/// One bad file never cancels the rest; their errors are returned together with the count.
#[cfg(test)]
pub fn run_files(
    inputs: &[std::path::PathBuf],
    steps: &[String],
    out_dir: &std::path::Path,
    suffix: &str,
) -> Result<String, String> {
    run_files_with(inputs, steps, out_dir, suffix, &|_, _| {})
}

/// `run_files`, reporting each file's state ("Processing", "Done", the error) as it goes.
pub fn run_files_with(
    inputs: &[std::path::PathBuf],
    steps: &[String],
    out_dir: &std::path::Path,
    suffix: &str,
    report: &dyn Fn(usize, String),
) -> Result<String, String> {
    if inputs.is_empty() {
        return Err("Add at least one audio file first.".into());
    }
    if steps.is_empty() {
        return Err("Choose a batch preset first.".into());
    }
    std::fs::create_dir_all(out_dir).map_err(|e| format!("Cannot create output folder: {e}"))?;
    let mut done = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        report(i, "Processing...".into());
        let output = crate::workflows::batch_output(input, out_dir, suffix);
        match run_batch(input, steps, &output) {
            Ok(_) => {
                done += 1;
                report(
                    i,
                    format!(
                        "Done: {}",
                        output
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    ),
                );
            }
            Err(e) => {
                report(i, format!("Error: {e}"));
                errors.push(format!(
                    "{}: {e}",
                    input
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                ));
            }
        }
    }

    let mut msg = format!(
        "Processed {done} of {} files into {}",
        inputs.len(),
        out_dir.display()
    );
    if !errors.is_empty() {
        msg.push_str("\n\n");
        msg.push_str(&errors.join("\n"));
    }
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_documented() {
        let mut ids: Vec<&str> = all().map(|c| c.id).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len(), "duplicate command id");
        assert!(all().all(|c| !c.doc.is_empty() && !c.title.is_empty()));
    }

    #[test]
    fn every_effect_command_resolves() {
        for c in COMMANDS.iter().filter(|c| c.id.starts_with("effect.")) {
            assert!(effect_by_id(c.id).is_some(), "{} has no effect", c.id);
        }
    }

    #[test]
    fn batch_runs_and_rejects_bad_input() {
        let dir = std::env::temp_dir().join("audiocraft_batch");
        std::fs::create_dir_all(&dir).unwrap();
        let inp = dir.join("in.wav");
        let out = dir.join("out.wav");
        let t = dsp::gen_tone(44100, 2, 1.0, 440.0, Wave::Sine, -12.0);
        audio::save_wav(&inp, &t, 16, false).unwrap();
        let steps: Vec<String> = [
            "process.normalize=-1",
            "favorite.podcast_voice",
            "effect.reverb",
            "process.stretch=50",
            "process.fade_out=0.1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        run_batch(&inp, &steps, &out).unwrap();
        let l = audio::load(&out).unwrap();
        assert!((l.data.duration() - 0.5).abs() < 0.01);
        // errors, not panics
        assert!(run_batch(&inp, &["no.such".to_string()], &out).is_err());
        assert!(run_batch(&inp, &["process.gain=abc".to_string()], &out).is_err());
        assert!(run_batch(&dir.join("missing.wav"), &[], &out).is_err());
    }

    #[test]
    fn batch_files_processes_good_files_and_reports_bad_ones() {
        let dir = std::env::temp_dir().join("audiocraft_batch_files");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("one.wav");
        let b = dir.join("two.wav");
        let t = dsp::gen_tone(8000, 1, 0.2, 440.0, Wave::Sine, -12.0);
        audio::save_wav(&a, &t, 16, false).unwrap();
        audio::save_wav(&b, &t, 16, false).unwrap();
        let out = dir.join("out");
        let steps: Vec<String> = vec!["process.normalize=-1".into()];
        let msg = run_files(&[a, b, dir.join("missing.wav")], &steps, &out, "_fixed").unwrap();
        assert!(msg.contains("Processed 2 of 3"));
        assert!(msg.contains("missing.wav"));
        assert!(out.join("one_fixed.wav").exists());
        assert!(out.join("two_fixed.wav").exists());
        assert!(run_files(&[], &steps, &out, "_x").is_err());
        assert!(run_files(&[dir.join("one.wav")], &[], &out, "_x").is_err());
    }
}
