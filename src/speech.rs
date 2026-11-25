//! Text-to-speech: turn typed words into a WAV and load it as a new audio file.
//!
//! On Windows this uses the built-in System.Speech engine (the same voices other Windows apps
//! use). On other systems it tries `espeak` and then `flite` if they are installed; if neither is
//! available, the caller gets a clear message instead of a crash. Nothing is sent off the
//! computer.

use crate::audio::{self, AudioData};
use std::path::{Path, PathBuf};
#[cfg(any(test, not(windows)))]
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Quote a string as a PowerShell single-quoted literal. A single quote is doubled; `$` and
/// backslashes stay literal, so text from the UI can never be interpreted as script code.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn temporary_wav() -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    crate::prefs::temp_dir().join(format!("audiocraft-speech-{}-{n}.wav", std::process::id()))
}

/// Ready voices: (name, gender, which installed voice of that gender, pitch in semitones).
/// Each picks a different real voice on the computer (the 1st, 2nd, 3rd woman's or man's voice;
/// "Hazel" and "Hazel Desktop" count once), so changing it changes who speaks. The pitch uses
/// the speech engine's own pitch control, which keeps the voice's character.
pub const PRESETS: [(&str, &str, usize, f32); 8] = [
    ("Woman", "Female", 0, 0.0),
    ("Woman 2", "Female", 1, 0.0),
    ("Woman 3", "Female", 2, 0.0),
    ("Man", "Male", 0, 0.0),
    ("Man 2", "Male", 1, 0.0),
    ("Deep Voice", "Male", 0, -3.0),
    ("High Voice", "Female", 1, 2.5),
    ("Bright Voice", "Female", 2, 4.0),
];

/// The preset a voice name means (`"preset:Woman"`), if any.
pub fn preset(voice: &str) -> Option<(&'static str, &'static str, usize, f32)> {
    let name = voice.strip_prefix("preset:")?;
    PRESETS.iter().copied().find(|p| p.0 == name)
}

/// The engine's pitch steps for a shift in semitones (SAPI `absmiddle`, -10..10 ≈ ±4.5 st).
pub fn sapi_pitch(semis: f32) -> i32 {
    (semis * 2.2).round().clamp(-10.0, 10.0) as i32
}

/// Every SAPI voice token on the computer, the classic and the newer (OneCore) ones.
const SAPI_TOKENS: &str = "$all = @(); foreach ($id in @('HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Speech\\Voices','HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Speech_OneCore\\Voices')) { try { $c = New-Object -ComObject SAPI.SpObjectTokenCategory; $c.SetId($id, $false); $ts = $c.EnumerateTokens(); for ($i = 0; $i -lt $ts.Count; $i++) { $all += $ts.Item($i) } } catch {} }";

/// The voices installed on this computer (Windows; asked once, on a worker thread, and empty
/// until the answer arrives or elsewhere).
pub fn installed_voices() -> Vec<String> {
    use std::sync::{Mutex, OnceLock};
    static LIST: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
    let cell = LIST.get_or_init(|| {
        #[cfg(all(windows, not(test)))]
        std::thread::spawn(|| {
            let script =
                format!("{SAPI_TOKENS}; $all | ForEach-Object {{ $_.GetAttribute('Name') }}");
            let mut cmd = crate::ffmpeg::command("powershell.exe");
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            let mut names: Vec<String> = cmd
                .output()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            names.dedup();
            if let Some(c) = LIST.get() {
                if let Ok(mut g) = c.lock() {
                    *g = Some(names);
                }
            }
        });
        Mutex::new(None)
    });
    cell.lock().ok().and_then(|g| g.clone()).unwrap_or_default()
}

#[cfg(any(test, not(windows)))]
/// Shift `d` by `semis` semitones keeping its length (time-stretch, then resample back). Only
/// used where the engine has no pitch control of its own (flite).
pub fn pitch_shift(d: &AudioData, semis: f32) -> AudioData {
    if semis.abs() < 0.01 || d.frames() < 64 {
        return d.clone();
    }
    let f = 2f64.powf(semis as f64 / 12.0);
    let n = d.frames();
    let mut longer = crate::dsp::stretch_range(d, 0, n, f);
    let rate = d.rate;
    longer.rate = ((rate as f64) * f).round().max(1.0) as u32;
    let mut out = audio::resample(&longer, rate);
    out.rate = rate;
    out
}

/// Text made safe inside the engine's XML markup.
fn xml_escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The PowerShell that speaks `text` through SAPI into `out` (48 kHz mono WAV).
fn windows_script(text: &str, voice: &str, rate: i32, semis: f32, out: &Path) -> String {
    let mut lines = vec![
        "$ErrorActionPreference = 'Stop'".to_string(),
        "$v = New-Object -ComObject SAPI.SpVoice".to_string(),
        SAPI_TOKENS.to_string(),
    ];
    let voice = voice.trim();
    if let Some((_, gender, idx, _)) = preset(voice) {
        lines.push(format!(
            "$seen = @{{}}; $g = @(); foreach ($t in $all) {{ if ($t.GetAttribute('Gender') -eq '{gender}') {{ $k = $t.GetAttribute('Name') -replace ' Desktop$',''; if (-not $seen.ContainsKey($k)) {{ $seen[$k] = 1; $g += $t }} }} }}; if ($g.Count -gt 0) {{ $v.Voice = $g[{idx} % $g.Count] }}"
        ));
    } else if !voice.is_empty() {
        lines.push(format!(
            "$t = $all | Where-Object {{ $_.GetAttribute('Name') -eq {} }} | Select-Object -First 1; if ($t) {{ $v.Voice = $t }}",
            ps_quote(voice)
        ));
    }
    lines.push(format!("$v.Rate = {}", rate.clamp(-10, 10)));
    lines.push("$fs = New-Object -ComObject SAPI.SpFileStream".to_string());
    lines.push("$fs.Format.Type = 38".to_string());
    lines.push(format!(
        "$fs.Open({}, 3, $false)",
        ps_quote(&out.to_string_lossy())
    ));
    lines.push("$v.AudioOutputStream = $fs".to_string());
    let total = semis + preset(voice).map(|p| p.3).unwrap_or(0.0);
    let markup = format!(
        "<pitch absmiddle=\"{}\">{}</pitch>",
        sapi_pitch(total),
        xml_escape(text)
    );
    lines.push(format!("[void]$v.Speak({}, 8)", ps_quote(&markup)));
    lines.push("$fs.Close()".to_string());
    lines.join("\n")
}

/// Build a local speech-engine command. Keeping construction separate lets the argument ordering
/// be tested on every platform without starting or requiring an external speech engine.
#[cfg(any(test, not(windows)))]
fn offline_speech_command(program: &str, flags: &[&str], out: &Path, text: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(flags).arg(out).arg(text);
    cmd
}

/// Speak `text` with a voice, rate (-10..10) and pitch shift in semitones (added to a ready
/// voice's own), into a temporary WAV, and return its samples. The temporary file is removed
/// even when loading fails, so repeated generations never fill the temp folder.
pub fn synthesize_pitched(
    text: &str,
    voice: &str,
    rate: i32,
    semis: f32,
) -> Result<AudioData, String> {
    synthesize_with(text, voice, rate, semis)
}

fn synthesize_with(text: &str, voice: &str, rate: i32, semis: f32) -> Result<AudioData, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Type the words to speak first.".into());
    }
    if text.chars().count() > 4000 {
        return Err("Please keep the speech text under 4000 characters.".into());
    }
    let out = temporary_wav();
    #[cfg(windows)]
    let attempted = {
        let script = windows_script(text, voice, rate, semis, &out);
        // the shared quiet command: no console window flashes while speaking
        let mut cmd = crate::ffmpeg::command("powershell.exe");
        cmd.arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(script);
        let result = cmd
            .output()
            .map_err(|e| format!("Could not start the Windows speech engine: {e}"))?;
        if !result.status.success() {
            let why = String::from_utf8_lossy(&result.stderr).trim().to_string();
            let why = if why.is_empty() {
                String::from_utf8_lossy(&result.stdout).trim().to_string()
            } else {
                why
            };
            return Err(format!(
                "The Windows speech engine could not speak that text: {why}"
            ));
        }
        true
    };
    #[cfg(not(windows))]
    let mut flite_used = false;
    #[cfg(not(windows))]
    let attempted = {
        // espeak takes a female/male variant and the rate; flite speaks as it is
        let espeak_voice = match preset(voice.trim()).map(|p| p.1) {
            Some("Female") => "en+f3",
            Some(_) => "en+m3",
            None => "en",
        };
        let speed = (175 + rate.clamp(-10, 10) * 12).to_string();
        let total = semis + preset(voice.trim()).map(|p| p.3).unwrap_or(0.0);
        let pitch = ((50.0 + total * 8.0).round().clamp(0.0, 99.0) as i32).to_string();
        let espeak_flags: Vec<&str> = vec!["-v", espeak_voice, "-s", &speed, "-p", &pitch, "-w"];
        const FLITE: &[&str] = &["-o"];
        let tools: Vec<(&str, &[&str])> = vec![("espeak", &espeak_flags[..]), ("flite", FLITE)];
        let mut last = String::new();
        let mut done = false;
        let mut flite_used = false;
        for &(program, flags) in &tools {
            let result = offline_speech_command(program, flags, &out, text)
                .stdin(std::process::Stdio::null())
                .output();
            match result {
                Ok(o) if o.status.success() => {
                    done = true;
                    flite_used = program == "flite";
                    break;
                }
                Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_string(),
                Err(e) => last = e.to_string(),
            }
        }
        if !done {
            return Err(format!(
                "No speech engine is installed. Install espeak or flite, or run AudioCraft on Windows. ({last})"
            ));
        }
        true
    };
    let _ = attempted;
    if !out.exists() {
        return Err("The speech engine ran but did not write an audio file.".into());
    }
    let loaded = audio::load(&out).map_err(|e| format!("The spoken audio could not be read: {e}"));
    let _ = std::fs::remove_file(&out);
    #[cfg(not(windows))]
    let loaded = loaded.map(|mut lf| {
        if flite_used {
            let total = semis + preset(voice.trim()).map(|p| p.3).unwrap_or(0.0);
            lf.data = pitch_shift(&lf.data, total);
        }
        lf
    });
    loaded.map(|lf| lf.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_engine_arguments_are_expanded_in_order() {
        let command = offline_speech_command(
            "espeak",
            &["-w", "--stdin"],
            Path::new("spoken words.wav"),
            "Hello from AudioCraft",
        );
        assert_eq!(command.get_program(), std::ffi::OsStr::new("espeak"));
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            [
                std::ffi::OsStr::new("-w"),
                std::ffi::OsStr::new("--stdin"),
                std::ffi::OsStr::new("spoken words.wav"),
                std::ffi::OsStr::new("Hello from AudioCraft"),
            ]
        );
    }

    #[test]
    fn presets_and_pitch_shift() {
        let s = windows_script("hi", "preset:Woman 2", 0, 0.0, Path::new("x.wav"));
        assert!(s.contains("GetAttribute('Gender') -eq 'Female'"));
        assert!(s.contains("$g[1 % $g.Count]"));
        let deep = windows_script("hi", "preset:Deep Voice", 0, 0.0, Path::new("x.wav"));
        assert!(deep.contains("absmiddle=\"-7\""));
        assert_eq!(sapi_pitch(100.0), 10);
        let mut d = AudioData::new(16000, 1, 16000);
        for (i, v) in d.ch[0].iter_mut().enumerate() {
            *v = (i as f32 * 2.0 * std::f32::consts::PI * 200.0 / 16000.0).sin() * 0.5;
        }
        let up = pitch_shift(&d, 12.0);
        assert!(
            (up.frames() as i64 - d.frames() as i64).abs() < 400,
            "length kept"
        );
        let crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let (a, b) = (
            crossings(&d.ch[0][2000..14000]),
            crossings(&up.ch[0][2000..14000]),
        );
        assert!(
            (b as f32 / a as f32 - 2.0).abs() < 0.2,
            "an octave up: {a} -> {b}"
        );
    }

    #[test]
    fn powershell_literals_cannot_break_out() {
        assert_eq!(ps_quote("plain"), "'plain'");
        assert_eq!(ps_quote("it's $env:windir"), "'it''s $env:windir'");
    }

    #[test]
    fn speech_script_uses_the_chosen_voice_and_escapes_text() {
        let out = Path::new("C:\\temp\\speech.wav");
        let s = windows_script("don't <stop> & go", "Ana's Voice", 3, 0.0, out);
        assert!(s.contains("$ErrorActionPreference = 'Stop'"));
        assert!(s.contains("GetAttribute('Name') -eq 'Ana''s Voice'"));
        assert!(s.contains("$v.Rate = 3"));
        assert!(s.contains("don''t &lt;stop&gt; &amp; go"));
        assert!(s.contains("C:\\temp\\speech.wav"));
    }

    /// Speaks for real with the computer's voices (Windows; run by hand with --ignored).
    #[test]
    #[ignore]
    fn real_voices_differ_and_pitch_moves() {
        let a = synthesize_pitched("Hello there, this is a test.", "preset:Woman", 0, 0.0).unwrap();
        let b = synthesize_pitched("Hello there, this is a test.", "preset:Man", 0, 0.0).unwrap();
        let c = synthesize_pitched("Hello there, this is a test.", "preset:Man", 0, -4.0).unwrap();
        println!(
            "woman {} man {} man-4 {}",
            a.frames(),
            b.frames(),
            c.frames()
        );
        assert!(a.ch[0] != b.ch[0], "a different voice speaks");
        assert!(b.ch[0] != c.ch[0], "the pitch changed the sound");
    }
}
