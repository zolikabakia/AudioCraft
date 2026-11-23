# AudioCraft commands

Every feature is a command with a stable ID. Run commands from the menus, from the keyboard, from the Command Palette (Ctrl+Shift+P), or list them with `audiocraft --list-commands`. This file is generated from that output.

| ID | Title | Shortcut | What it does |
|---|---|---|---|
| `file.new` | New Audio File | Ctrl+N | Create an empty audio file (asks for sample rate, channels and bit depth). |
| `file.new_session` | New Multitrack Session | Ctrl+Shift+N | Create an empty multitrack session. |
| `file.open` | Open | Ctrl+O | Open a session or AudioCraft project first; audio files and videos are also available. |
| `file.import_video` | Import Video File... | Ctrl+Shift+I | Import a video in the background, show its frames in the Video panel and play its audio. |
| `file.import_folder` | Import Folder... |  | Import every audio file, video and session in a folder and its sub-folders (dragging a folder in does the same). |
| `video.detect_decoder` | Detect Video Decoder |  | Find the bundled free FFmpeg decoder, or one in the configured folders and the system PATH. |
| `file.save` | Save | Ctrl+S | Save the current file as WAV, or the session in Multitrack. |
| `file.save_as` | Save As | Ctrl+Shift+S | Save under a new name. |
| `file.close` | Close File | Ctrl+W | Close the current file. |
| `file.export_mixdown` | Export Multitrack Mixdown... |  | Export the whole session as one audio file, with format and sample type choices. |
| `file.batch_process` | Batch Processor... |  | Run a preset over many files and write the results into an output folder. |
| `file.export_report` | Export Analysis Report... |  | Save peak, RMS and loudness estimates of the current file as CSV. |
| `edit.undo` | Undo | Ctrl+Z | Undo the last change of the current file or session. |
| `edit.redo` | Redo | Ctrl+Shift+Z | Redo the last undone change. |
| `edit.cut` | Cut | Ctrl+X | Cut the selection to the clipboard. |
| `edit.copy` | Copy | Ctrl+C | Copy the selection to the clipboard. |
| `edit.paste` | Paste | Ctrl+V | Insert the clipboard at the playhead (replaces the selection). |
| `edit.mix_paste` | Mix Paste | Ctrl+Shift+V | Mix the clipboard over the audio at the playhead. |
| `edit.paste_new` | Paste to New File | Ctrl+Alt+V | Create a new file from the clipboard. |
| `edit.delete` | Delete | Del | Delete the selection, or the selected clip in Multitrack. |
| `edit.crop` | Crop | Ctrl+T | Keep only the selection. |
| `edit.select_all` | Select All | Ctrl+A | Select the whole file or session. |
| `edit.deselect` | Deselect All | Ctrl+Shift+A | Clear the selection. |
| `edit.insert_silence` | Insert Silence |  | Insert silence at the playhead. |
| `edit.convert` | Convert Sample Type | Shift+T | Change sample rate, channels or bit depth. |
| `edit.preferences` | Preferences | Ctrl+Shift+K | The Preferences window: General, Appearance, Audio Hardware, Multitrack (keyframe and handle sizes), Spectral Displays and more. |
| `tool.direct_selection` | Direct Selection Tool | A | The white arrow: keyframes show their curve handles. Drag a handle to bend the curve, Alt+drag a handle to move it alone, Alt+drag a keyframe to pull out new handles, Alt+click a keyframe to add or remove its handles. |
| `keyframes.delete` | Delete Selected Keyframes | Del | Remove the selected keyframes (click a keyframe to select it; Shift or Ctrl adds to the selection). |
| `keyframes.deselect` | Deselect Keyframes | Esc | Clear the keyframe selection. |
| `session.convert_for_audition` | Convert Curves for Audition |  | Turn curved keyframes into ordinary keyframes and shaped fades into the clip volume envelope, so a .sesx file holds the session exactly (one Undo takes it back). |
| `view.spectral_log` | Logarithmic Spectral Scale |  | Switch the Spectral Frequency Display between a linear and a logarithmic frequency scale. |
| `view.spectral_pitch` | Show Spectral Pitch Display |  | Show or hide the Spectral Pitch Display (notes on the scale, the detected pitch as a blue line). |
| `files.play` | Play Selected File |  | Play (or stop) the selected file in the Files panel's own preview player; never moves the editor playhead. |
| `files.loop` | Loop File Playback |  | Loop the Files panel's independent preview player. |
| `files.autoplay` | Auto-Play Files |  | Play each file in the file browser as it is selected, without moving the playhead. |
| `marker.add` | Add Marker | M | Add a marker at the playhead (or a range marker for the selection). |
| `marker.next` | Go to Next Marker | PgDn | Move the playhead to the next marker (wraps around). |
| `marker.previous` | Go to Previous Marker | PgUp | Move the playhead to the previous marker (wraps around). |
| `marker.rename` | Rename Marker... |  | Rename the marker nearest the playhead. |
| `marker.delete` | Delete Marker |  | Delete the marker nearest the playhead. |
| `marker.export` | Export Markers... |  | Save markers as a small CSV file (name, start, duration). |
| `marker.import` | Import Markers... |  | Add markers from a CSV file with name, start and duration columns. |
| `transport.play` | Play / Stop | Space | Start or stop playback. |
| `transport.pause` | Pause |  | Pause playback and keep the playhead where it is. |
| `transport.record` | Record | Shift+Space | Record from the input device. |
| `transport.loop` | Toggle Loop |  | Loop the selection (or the whole file) during playback. |
| `transport.play_selection` | Play Selection |  | Play only the selected range, looping until stopped. |
| `transport.return_play` | Return to Start and Play |  | Move the playhead to the beginning and play. |
| `transport.to_start` | Playhead to Start | Home | Move the playhead to the start. |
| `transport.to_end` | Playhead to End | End | Move the playhead to the end. |
| `view.waveform` | Waveform Editor | 8 | Switch to the Waveform editor. |
| `view.multitrack` | Multitrack Editor | 9 | Switch to the Multitrack editor. |
| `view.mixer` | Mixer |  | Show the Mixer. |
| `view.spectral` | Spectral Frequency Display | Shift+D | Show or hide the spectral display. |
| `view.pitch_display` | Spectral Pitch Display |  | Show or hide the pitch of the sound over time (Waveform Editor). |
| `window.workspace_reset` | Reset Workspace to Saved Layout |  | Put the panels of the current workspace back where they were saved. |
| `window.workspace_save` | Save Changes to this Workspace |  | Keep the current panel arrangement as this workspace. |
| `view.zoom_in` | Zoom In | = | Zoom in on time. |
| `view.zoom_out` | Zoom Out | - | Zoom out on time. |
| `view.zoom_full` | Zoom Out Full | \ | Show the whole file or session. |
| `view.zoom_selection` | Zoom to Selection |  | Fit the selection in the view. |
| `view.envelopes` | Show Clip Volume Envelopes |  | Show or hide the yellow volume lines on clips. |
| `view.pan_envelopes` | Show Clip Pan Envelopes |  | Show or hide the blue pan lines on clips (click to add a point, drag to move, double-click to remove). |
| `window.clip_effects` | Clip Effects |  | Show the Effects Rack for the selected clip. |
| `window.track_effects` | Track Effects |  | Show the Effects Rack for the selected track. |
| `tool.move` | Move Tool | V | Move clips in Multitrack. |
| `tool.razor` | Razor Tool | R | Split clips where you click (Shift splits all tracks). |
| `tool.slip` | Slip Tool | Y | Slide the audio inside a clip. |
| `tool.time` | Time Selection Tool | T | Select a time range. |
| `tool.marquee` | Marquee Selection Tool | E | Spectral display: drag a rectangle of time and frequency; drag inside it to move it, drag its edges to resize it. |
| `tool.lasso` | Lasso Selection Tool | D | Spectral display: draw a free-form selection of time and frequency. |
| `tool.brush` | Paintbrush Selection Tool | P | Spectral display: paint a selection (Shift adds to it). |
| `tool.spot_heal` | Spot Healing Brush Tool | B | Spectral display: paint over a click or noise to heal it away. |
| `spectral.silence` | Silence Spectral Selection |  | Remove the sound inside the spectral selection. |
| `spectral.heal` | Heal Spectral Selection |  | Replace the sound inside the spectral selection with the level around it. |
| `spectral.deselect` | Deselect Spectral Selection |  | Clear the spectral selection. |
| `spectral.invert` | Select Inverse (Spectral) |  | Select the other frequencies over the same time range as the spectral selection. |
| `spectral.attenuate` | Attenuate Spectral Selection |  | Turn the selected frequencies down by 12 dB. |
| `spectral.select_all` | Select All Frequencies |  | Select the whole time and frequency range of the spectral display. |
| `view.toggle_metronome` | Toggle Metronome |  | Show or hide the metronome row in Multitrack (its clicks play with the session). |
| `view.toggle_clip_stretch` | Toggle Global Clip Stretching |  | Show stretch handles on every clip's top corners; drag them to change a clip's length (the % shows the new length). |
| `view.toggle_snapping` | Toggle Snapping |  | Snap clips, drops and the playhead to clip edges, the playhead and the session start. |
| `view.toggle_auto_scroll` | Toggle Playback Auto-Scroll |  | Keep the playhead in view while playing. |
| `view.theme_toggle` | Next Colour Theme |  | Switch to the next colour theme (AudioCraft, Audition Dark, then the PhotoCraft and VectorCraft themes). |
| `view.track_header_io` | Track Headers: Inputs/Outputs |  | Show each track's input and output in its header. |
| `view.track_header_fx` | Track Headers: Effects |  | Show each track's effects in its header. |
| `view.track_header_sends` | Track Headers: Sends |  | Show each track's send in its header. |
| `view.track_header_eq` | Track Headers: EQ |  | Show each track's EQ in its header (the pencil opens the Track EQ). |
| `view.spectral_zoom_reset` | Reset Frequency Zoom |  | Show the whole frequency range in the spectral display. |
| `clip.split` | Split at Playhead | Ctrl+K | Split the selected clip (or all clips) at the playhead. |
| `clip.split_markers` | Split Clips at Markers |  | Split every clip that crosses a marker. |
| `clip.duplicate` | Duplicate Clip | Ctrl+D | Copy the selected clip to start just after itself. |
| `clip.fade_in` | Fade Selected Clip In |  | Apply a 0.25 s fade at the start of the selected clip. |
| `clip.fade_out` | Fade Selected Clip Out |  | Apply a 0.25 s fade at the end of the selected clip. |
| `clip.crossfade` | Crossfade with Next Clip |  | Turn the overlap with the next clip into a balanced crossfade. |
| `clip.mixdown_new` | Mixdown Session to New File |  | Render the session into a new file. |
| `track.add_stereo` | Add Stereo Track |  | Add a stereo track to the session. |
| `track.add_mono` | Add Mono Track |  | Add a mono track to the session. |
| `track.duplicate` | Duplicate Track | Ctrl+Shift+D | Copy the selected track and all its clips. |
| `track.delete` | Delete Track |  | Remove the selected track and its clips. |
| `track.move_up` | Move Track Up |  | Move the selected track one place up. |
| `track.move_down` | Move Track Down |  | Move the selected track one place down. |
| `clip.match_loudness` | Match Clip Loudness |  | Set the gain of every clip on the selected track so they all play at the same average loudness. |
| `help.settings_folder` | Open Settings Folder |  | Show the folder where preferences, workspaces and recent files are kept. |
| `track.rename` | Rename Track... |  | Change the name of the selected track. |
| `track.mute_all` | Mute All Tracks |  | Mute or unmute every track at once. |
| `track.solo_clear` | Clear All Solos |  | Turn off solo on every track. |
| `view.volume_automation` | Show/Hide Volume Automation Lane | Alt+A | Use the chevron beside the Read selector to open or close the selected track's volume-automation lane. |
| `track.automation_point` | Add Volume Automation Point |  | Add a volume-automation point at the playhead on the selected track (or Master). |
| `track.automation_clear` | Clear Volume Automation |  | Remove every volume-automation point from the selected track (or Master). |
| `window.essential_sound` | Essential Sound | Shift+E | Show or hide the Essential Sound panel. |
| `window.effects_rack` | Effects Rack |  | Show the Effects Rack. |
| `window.markers` | Markers |  | Show the Markers panel. |
| `window.amplitude_statistics` | Amplitude Statistics |  | Show peak, RMS and loudness numbers. |
| `window.frequency_analysis` | Frequency Analysis |  | Show the frequency spectrum. |
| `window.command_palette` | Command Palette | Ctrl+Shift+P | Search and run any command. |
| `process.invert` | Invert |  | Flip the polarity of the selection. |
| `process.reverse` | Reverse |  | Play the selection backwards. |
| `process.silence` | Silence |  | Replace the selection with silence. |
| `process.remove_dc` | DC Offset |  | Remove DC offset. |
| `process.gain` | Gain... | Ctrl+G | Raise or lower the selection by a number of dB. |
| `process.declip` | Repair Clipped Peaks |  | Rebuild flat-topped peaks from the audio around them. |
| `process.normalize` | Normalize |  | Raise the peak to a target level. |
| `process.fade_in` | Fade In |  | Fade in the selection. |
| `process.fade_out` | Fade Out |  | Fade out the selection. |
| `process.match_loudness` | Match Loudness |  | Set the loudness to a target in LUFS. |
| `process.stretch_pitch` | Stretch and Pitch |  | Time-stretch keeping pitch, pitch-shift keeping length, or varispeed. |
| `process.capture_noise_print` | Capture Noise Print |  | Learn the noise from the selection. |
| `process.noise_reduction` | Noise Reduction |  | Remove the captured noise. |
| `generate.tones` | Generate Tones |  | Insert a test tone. |
| `generate.speech` | Generate Speech... |  | Speak text into a new audio file using an installed voice (Windows SAPI, or espeak/flite on other systems). |
| `generate.dtmf` | Generate DTMF Tones |  | Insert telephone dial tones for a dial string. |
| `process.auto_heal` | Auto Heal Selection | Ctrl+U | Rebuild a short selection (click, cough) from the audio around it. |
| `process.fade_envelope` | Fade Envelope |  | Shape the level of the selection with a drawn curve. |
| `process.gain_envelope` | Gain Envelope |  | Change the gain across the selection with a drawn dB curve. |
| `process.delete_silence` | Delete Silence |  | Remove quiet passages. |
| `process.mark_audio` | Mark Audio |  | Add markers where sound starts after a pause. |
| `process.click_pop` | Click/Pop Eliminator |  | Find and redraw clicks and pops in the selection. |
| `process.phase_correction` | Automatic Phase Correction |  | Line up the right channel with the left. |
| `process.pitch_bender` | Pitch Bender |  | Bend pitch and speed from a start to an end value. |
| `window.plugin_manager` | Audio Plug-In Manager |  | List built-in and installed VST3/CLAP effects and scan for new ones. |
| `window.plugins` | Plug-ins |  | Show or hide the Plug-ins panel, where VST3 and CLAP effects open their own windows (docked at the right). |
| `plugins.pop_out` | Pop Out / Dock Plug-in Window |  | Move the front plug-in editor from the Plug-ins panel into its own floating window, or dock a floating one back. |
| `plugins.close_all` | Close All Plug-in Windows |  | Close every open VST3/CLAP editor; each plug-in keeps its settings. |
| `plugins.toggle_dock` | Dock Plug-in Windows in the Panel |  | Open VST3/CLAP editors in the Plug-ins panel, or in their own floating windows. |
| `effect.phaser` | Phaser |  | Sweeping all-pass phaser. |
| `effect.chorus_flanger` | Chorus/Flanger |  | Chorus or flanger in one effect. |
| `effect.analog_delay` | Analog Delay |  | Delay with a darkening, saturating feedback path. |
| `effect.echo` | Echo |  | Separate left/right echoes with tone control. |
| `effect.deesser` | DeEsser |  | Turn down harsh sibilance. |
| `effect.multiband_compressor` | Multiband Compressor |  | Three-band compressor. |
| `effect.dynamics_processing` | Dynamics Processing |  | Expander, compressor and limiter in one. |
| `effect.tube_modeled_compressor` | Tube-modeled Compressor |  | Compressor with warm saturation. |
| `effect.speech_volume_leveler` | Speech Volume Leveler |  | Even out spoken levels automatically. |
| `effect.channel_mixer` | Channel Mixer |  | Remix the left and right channels. |
| `effect.graphic_equalizer` | Graphic Equalizer |  | 10, 20 or 30 band graphic EQ. |
| `effect.notch_filter` | Notch Filter |  | Cut up to six narrow frequencies. |
| `effect.scientific_filter` | Scientific Filter |  | Butterworth low/high/band pass or band stop. |
| `effect.fft_filter` | FFT Filter |  | Draw any frequency response (linear phase). |
| `effect.studio_reverb` | Studio Reverb |  | Pre-delay, early reflections and a damped tail. |
| `effect.full_reverb` | Full Reverb |  | Studio reverb with stronger early reflections. |
| `effect.convolution_reverb` | Convolution Reverb |  | Reverb from impulse responses (rooms, halls, plate). |
| `effect.guitar_suite` | Guitar Suite |  | Compressor, distortion, tone and cabinet. |
| `effect.mastering` | Mastering |  | EQ, exciter, widener and loudness maximizer. |
| `effect.vocal_enhancer` | Vocal Enhancer |  | Clarity for male, female or music voices. |
| `effect.center_channel_extractor` | Center Channel Extractor |  | Keep or remove what is in the centre of the stereo image. |
| `effect.graphic_phase_shifter` | Graphic Phase Shifter |  | Shift the phase of each channel. |
| `effect.adaptive_noise_reduction` | Adaptive Noise Reduction |  | Removes steady noise it learns while playing. |
| `effect.denoise` | DeNoise |  | Gentle broadband noise reduction. |
| `effect.hiss_reduction` | Hiss Reduction |  | Reduce hiss above a frequency. |
| `effect.dereverb` | DeReverb |  | Reduce room reverberation. |
| `effect.automatic_click_remover` | Automatic Click Remover |  | Remove clicks and crackle in real time. |
| `generate.noise` | Generate Noise |  | Insert white, pink or brown noise. |
| `effect.amplify` | Amplify |  | Change the level. |
| `effect.parametric_eq` | Parametric Equalizer |  | Six-band EQ with a draggable curve. |
| `effect.simple_filter` | Simple Filter |  | Low/high/band pass, notch, peak and shelves. |
| `effect.compressor` | Single-band Compressor |  | Even out the dynamics. |
| `effect.hard_limiter` | Hard Limiter |  | Stop peaks above a ceiling. |
| `effect.noise_gate` | Noise Gate |  | Silence quiet parts. |
| `effect.dehummer` | DeHummer |  | Remove 50/60 Hz hum and harmonics. |
| `effect.delay` | Delay |  | Echo with feedback. |
| `effect.reverb` | Reverb |  | Room reverb. |
| `effect.chorus` | Chorus |  | Thicken with modulated delays. |
| `effect.flanger` | Flanger |  | Sweeping comb-filter effect. |
| `effect.distortion` | Distortion |  | Soft-clipping saturation. |
| `effect.stereo_expander` | Stereo Expander |  | Widen or narrow the stereo image. |
| `favorite.fade_in` | Favorite: Fade In |  | Cosine fade in. |
| `favorite.fade_out` | Favorite: Fade Out |  | Cosine fade out. |
| `favorite.normalize_0` | Favorite: Normalize to -0.1 dB |  | Normalize to -0.1 dB. |
| `favorite.normalize_3` | Favorite: Normalize to -3 dB |  | Normalize to -3 dB. |
| `favorite.reduce_rumble` | Favorite: Reduce Rumble |  | High-pass at 80 Hz. |
| `favorite.telephone` | Favorite: Telephone Voice |  | Band-limited telephone sound. |
| `favorite.podcast_voice` | Favorite: Podcast Voice |  | EQ, compression and limiting for speech. |
| `favorite.light_reverb` | Favorite: Light Reverb |  | A touch of room. |
| `help.shortcuts` | Keyboard Shortcuts |  | List the keyboard shortcuts. |
| `help.about` | About AudioCraft |  | Version information. |
| `file.open_append_new` | Open Append: To New... |  | Open several audio files one after another as one new file (converted to the first file's sample rate and channels). |
| `file.open_append_current` | Open Append: To Current... |  | Add audio files to the end of the current file (one Undo takes it back). |
| `file.open_selected` | Open in Waveform Editor |  | Open the file selected in the Files list in the Waveform editor. |
| `file.insert_selected` | Insert into Multitrack |  | Put the file selected in the Files list on a track at the playhead, creating a session when one is not open. |
| `file.close_selected` | Close Selected File |  | Close the file selected in the Files list, asking first if it has unsaved changes. |
| `file.close_all` | Close All |  | Close every file and the session; anything unsaved is asked about first (Yes, Yes To All, No, No To All, Cancel). |
| `file.close_unused` | Close Unused Media |  | Close the files that no clip of the session uses. |
| `file.close_session_media` | Close Session and Its Media |  | Close the session and the files its clips use; unsaved changes are asked about first. |
| `file.save_selection_as` | Save Selection As... |  | Save only the selected range of the current file as a new WAV file. |
| `file.save_all` | Save All |  | Save every file and the session that have unsaved changes. |
| `file.revert` | Revert to Saved |  | Throw away the changes to the current file and load it again from disk (one Undo takes it back). |
| `file.reveal` | Reveal in Explorer |  | Show the current file (or the session) in the system file browser. |
| `edit.repeat_last` | Repeat Last Command | Ctrl+R | Run the last command again (a process, effect, favorite or generator). |
| `edit.copy_to_new` | Copy to New | Alt+Shift+C | Copy the selection into a new file. |
| `edit.select_view` | Select Current View |  | Select the time range that is showing. |
| `edit.select_to_start` | Select Playhead to Start | Shift+Home | Select from the start to the playhead. |
| `edit.select_to_end` | Select Playhead to End | Shift+End | Select from the playhead to the end. |
| `edit.select_track_clips` | Select All Clips in Selected Track |  | Select every clip of the selected track. |
| `edit.ripple_delete_clips` | Ripple Delete: Selected Clips |  | Delete the selected clips and pull the later clips of their tracks left to close the gap. |
| `edit.ripple_delete_time` | Ripple Delete: Time Selection in All Tracks |  | Cut the time selection out of every track and pull everything after it left. |
| `edit.zero_cross_in` | Zero Crossings: Adjust Selection Inward |  | Move both ends of the selection inward to the nearest zero crossing (clean cuts without clicks). |
| `edit.zero_cross_out` | Zero Crossings: Adjust Selection Outward |  | Move both ends of the selection outward to the nearest zero crossing. |
| `edit.enable_left` | Enable Channels: Left |  | Turn editing of the left channel on or off (edits and effects leave a disabled channel alone). |
| `edit.enable_right` | Enable Channels: Right |  | Turn editing of the right channel on or off. |
| `edit.extract_mono` | Extract Channels to Mono Files |  | Make a mono file from each channel of the current file. |
| `edit.interpret_sample_rate` | Interpret Sample Rate... |  | Change the file's declared rate without resampling: the same samples now play faster or slower, changing pitch and duration. |
| `edit.frequency_band_splitter` | Frequency Band Splitter... |  | Split the file (or selection) into low, mid and high band files at adjustable crossovers. |
| `edit.keyboard_shortcuts` | Keyboard Shortcuts... | Alt+K | List the keyboard shortcuts. |
| `multitrack.insert_files` | Insert Files... |  | Open audio files and put them on the selected track and the ones under it, at the playhead. |
| `multitrack.bounce_track` | Bounce to New Track: Selected Track |  | Render the selected track (clips, effects, volume) into a new file on a new track. |
| `multitrack.bounce_selection` | Bounce to New Track: Time Selection |  | Render the time selection of the whole session into a new file on a new track. |
| `clip.trim_selection` | Trim to Time Selection |  | Cut the selected clip down to the time selection. |
| `clip.trim_start` | Trim Start to Playhead | Alt+[ | Move the selected clip's start to the playhead. |
| `clip.trim_end` | Trim End to Playhead | Alt+] | Move the selected clip's end to the playhead. |
| `clip.mute` | Mute Clip |  | Mute or unmute the selected clip. |
| `clip.edit_source` | Edit Source File |  | Open the selected clip's file in the Waveform Editor. |
| `clip.edit_original` | Edit Original |  | Open the selected clip's original file in the default application for its file type. |
| `view.zoom_in_amp` | Zoom In Amplitude | Alt+= | Make the waveform taller. |
| `view.zoom_out_amp` | Zoom Out Amplitude | Alt+- | Make the waveform shorter. |
| `view.zoom_reset_amp` | Reset Amplitude Zoom |  | Show the waveform at its normal height. |
| `file.export_file` | Export Audio File... | Ctrl+Shift+E | Export the current file: name, folder, format (WAV, AIFF, FLAC, MP3, AAC), sample rate, channels, bit depth and metadata. |
| `file.export_mixdown_selection` | Export Multitrack Mixdown: Time Selection... |  | Export the session's time selection as one audio file, with format and sample type choices. |
| `file.export_settings` | Export Application Settings... |  | Save the preferences, workspaces, favorites and other settings into one file (to move them to another computer). |
| `file.import_settings` | Import Application Settings... |  | Load settings saved with Export Application Settings. |
| `process.doppler` | Doppler Shifter (process)... |  | Make the selection sound like it passes by in a straight line or circles around you. |
| `process.sound_remover` | Sound Remover (process)... |  | Learn a sound from a selection, then remove it from the file. |
| `help.manual` | AudioCraft Help | F1 | A short guide to the editors, the panels and where to find each command. |
| `window.new_window` | New Window |  | Open a new, empty window for panels; add panels to it with its Add Panel menu. Closing it docks its panels back. |
| `window.dock_all` | Dock All Windows |  | Put the panels of every separate window back into the main window. |
| `help.search` | Search Help |  | Search every command by name and description; pick one to run it or read its help. |
| `transport.record_ready` | Record (Ready) | Shift+Space | Get recording ready on the armed tracks (or the open file); recording starts when you press Play. Pressing it while playing punches in at once. |
| `video.decoders` | Video Decoders... |  | Choose which installed video decoder is used, or download a free one (Edit > Preferences > Video). |
| `video.convert_color` | Convert Video Preview to Rec.709 |  | Re-decode the video's preview frames converted from wide-gamut / HDR colour to standard Rec.709. |

## More menus (clipboards, metronome, clip gain)

| ID | Title | Shortcut | What it does |
|---|---|---|---|
| `edit.clipboard_1` | Set Current Clipboard 1 |  | Use clipboard 1 for Cut, Copy and Paste (each of the five clipboards keeps its own audio). |
| `edit.clipboard_2` | Set Current Clipboard 2 |  | Use clipboard 2 for Cut, Copy and Paste. |
| `edit.clipboard_3` | Set Current Clipboard 3 |  | Use clipboard 3 for Cut, Copy and Paste. |
| `edit.clipboard_4` | Set Current Clipboard 4 |  | Use clipboard 4 for Cut, Copy and Paste. |
| `edit.clipboard_5` | Set Current Clipboard 5 |  | Use clipboard 5 for Cut, Copy and Paste. |
| `metronome.beats_2` | Metronome: 2 Beats per Bar |  | Accent every second click. |
| `metronome.beats_3` | Metronome: 3 Beats per Bar |  | Accent every third click (3/4). |
| `metronome.beats_4` | Metronome: 4 Beats per Bar |  | Accent every fourth click (4/4). |
| `metronome.beats_6` | Metronome: 6 Beats per Bar |  | Accent every sixth click (6/8). |
| `metronome.tempo_up` | Metronome: Faster Tempo |  | Raise the session tempo by 5 BPM (up to 300). |
| `metronome.tempo_down` | Metronome: Slower Tempo |  | Lower the session tempo by 5 BPM (down to 20). |
| `clip.gain_up` | Clip Gain +1 dB |  | Raise the selected clips' gain by 1 dB. |
| `clip.gain_down` | Clip Gain -1 dB |  | Lower the selected clips' gain by 1 dB. |
| `clip.gain_reset` | Reset Clip Gain |  | Set the selected clips' gain back to 0 dB. |
| `clip.bring_front` | Bring Clip to Front |  | Draw and play the selected clip on top of clips it overlaps on its track. |
| `clip.fade_in_remove` | Remove Fade In |  | Take the fade-in off the selected clips. |
| `clip.fade_out_remove` | Remove Fade Out |  | Take the fade-out off the selected clips. |
| `clip.group` | Group Clips |  | Group the selected clips: they then select, move and delete together. |
| `clip.ungroup` | Ungroup Clips |  | Take the selected clips out of their group. |
| `tracks.show_all` | Show All Tracks |  | Show every track in the Editor (Tracks panel). |
| `tracks.hide_all` | Hide All Tracks |  | Hide every track from the Editor; hidden tracks still play. |
| `tracks.only_selected` | Show Only Selected Track |  | Hide every track except the selected one. |
| `tracks.hide_empty` | Hide Empty Tracks |  | Hide tracks that hold no clips. |
