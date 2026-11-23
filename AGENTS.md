# Working on AudioCraft (agents and humans)

- **Clean room:** use only public documentation and shortcut lists of other audio editors. Never inspect,
  decompile or copy another product's files, code or artwork.
- **Never crash:** return `Result`/errors instead of panicking. Avoid `unwrap()` on user data, and avoid
  `clamp` with bounds that can cross. Every crash fix ships with a regression test.
- **Commands:** every feature is a command in `src/commands.rs` with an ID, title, shortcut and doc,
  runnable through `App::exec`. Wire it to a menu or shortcut, then regenerate `docs/COMMANDS.md`
  from `audiocraft --list-commands`.
- **Licensing:** any added asset needs a clear licence and an attribution in `README.md` in the same change.
- **Running tests:** only `cargo test --target-dir target-alt -- --test-threads=1`, and kill any run
  that passes 120 s. Tests never scan or load real plug-ins: hosting is off under `cfg(test)` and
  the hosts search an empty temporary folder. A test that needs a real plug-in gets `#[ignore]`.
- **Plug-in hosting:** `AUDIOCRAFT_NO_PLUGIN_SCAN=1` turns CLAP/VST3 hosting off;
  `AUDIOCRAFT_PLUGIN_PATH` (a PATH-style list) replaces the standard plug-in folders. On exit the
  app stops audio, drops every instance (each terminates its component/controller), runs the VST3
  module exits (`ExitDll`) on the main thread, and never unloads a plug-in DLL.
- **Definition of done:** registered command, tests (including undo/redo where it edits audio), menu
  or shortcut, checked by screenshot (`audiocraft --screenshot out.png [--demo] [--multitrack|--mixer]`),
  `cargo build` and `cargo test` passing, docs updated.
