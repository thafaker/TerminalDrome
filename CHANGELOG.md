# Changelog

All notable changes to TerminalDrome are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions use
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as adapted for a
`0.x` series: the minor digit marks a release that may change behaviour users
can notice, the patch digit stays within that line.

## [0.9.7] — 2026-09-29

The lyrics view now follows the song. A track whose lyrics carry timestamps
gets the line that is being sung marked and the view scrolling to keep it in
sight, so the words can be read along with the music instead of hunted for.

The endpoint that supplies those timestamps is the part most people will never
notice and the part that had to change. Navidrome's long-standing `getLyrics`
answers with the words and strips the timings, so a client asking only that
one can never follow a line along no matter how well it renders. The timings
live in `structuredLyrics`, which reaches clients through `getLyricsBySongId`.
TerminalDrome asks for that first and keeps the old endpoint as a fallback, so
servers that do not have the OpenSubsonic extension see no change at all.

Lyrics that arrive without timestamps are now labelled as such. They are shown
exactly as before, but a track that has never had timed lyrics available
otherwise looks identical to a feature that is not working, and the reader has
no way to tell those apart from the screen.

### Added

- The lyrics view marks the line being sung, dims the lines already past, and
  scrolls itself to keep the sung line in sight. Opened with `Shift+Y`.
- Lyrics without timestamps say so in the title bar, so a still page reads as
  a property of the track rather than a fault in the client.
- LRC parsing for `[mm:ss.xx]` and `[mm:ss:xx]` timestamps, several timestamps
  on one line, unsorted lines, and section markers such as `[Chorus]`.

### Changed

- Lyrics are requested from the endpoint that carries the timing information,
  falling back to `getLyrics` for servers without the OpenSubsonic extension.
  A server that answers only the older endpoint keeps working unchanged, and
  no track loses lyrics it could be shown before.
- A line stays marked until the next one begins, rather than being cleared at
  its own timestamp, and nothing is marked before the first timestamp, so a
  track opening with instrumental music does not show its first line early.
- Scrolling the lyrics by hand stops the view from following the song and says
  so in the hint line. `Home` resumes following.

### Fixed

- Switching source with `Shift+B` no longer loses the place. Each source now
  remembers the view it was left in and the position inside its lists, so
  coming back lands on the same artist, album or playlist at the same spot
  rather than at the top of the artist list. Each source keeps its own, and
  neither overwrites the other.
- Long lyrics are measured in screen rows rather than in source lines, so a
  line that wraps over several rows no longer makes the view follow to the
  wrong place.
- The lyrics title bar and position counter are no longer clipped on narrow
  terminals: the bar now gives up one whole part at a time rather than cutting
  one in half.

## [0.9.6] — 2026-09-27

The audio visualizer now says so when it is not actually reacting to the
music. Previously a missing `cava` produced an animated demo that was
indistinguishable from a working analyser, so there was no way to tell from
the screen whether your music was driving the bars.

### Added

- The visualizer labels its bars while they are a demo animation, naming the
  program that is missing. The label disappears as soon as real audio levels
  arrive, so a working setup shows nothing extra.
- A first-run hint about `cava` at the end of the setup wizard.

### Changed

- The wizard and the README resolve the install command from `/etc/os-release`
  instead of assuming Arch. Distributions that share a package manager inherit
  it through `ID_LIKE`, so Debian derivatives resolve to `apt` without being
  enumerated, and an unrecognised distribution is told to use its own package
  manager rather than being sent a command that does not exist on it.
- A starting backend is given a short grace period before it is reported as
  broken, so a correctly working setup no longer flashes a warning.
- README installation instructions for `mpv` and `cava` cover macOS,
  Debian/Ubuntu, Fedora, Arch and NixOS.

## [0.9.5] — 2026-09-27

The first release since 0.9.0. Several fixes make server-side problems visible
that used to be silent, so read the **Changed** section before upgrading — a
connection that used to look like an empty collection will now report an error.

Versions 0.9.1 to 0.9.4 were never published and left no artefacts. The version
number was bumped in `Cargo.toml` while the work was still in progress, but no
tag, no GitHub release and no crates.io upload was ever made for them, so
nothing was superseded. They are skipped rather than reserved.

### Added

- `terminaldrome --about` prints a longer description of the project, its
  author and its license. It works before anything is configured, so it does
  not trigger the setup wizard.
- `TERMINALDROME_COVER` selects how cover art is rendered:
  - `blocks` — five tonal steps (default)
  - `ascii` — ten tonal steps, pure ASCII
  - `halfblock` — two pixels per cell, sharper edges, but binary per sub-pixel

  All three modes occupy the same grid, so switching never changes the layout.
- `[bandcamp] enabled = false` keeps stored credentials while switching the
  optional source off. It defaults to `true`, so existing configurations are
  unaffected.
- A `LICENSE` file is now shipped in the package. Earlier releases declared MIT
  in their metadata but shipped no license text.

### Fixed

- Subsonic responses are validated instead of being taken at face value. A
  failed response envelope, a body that is not Subsonic at all, and an HTML
  error page from a proxy are now reported as errors rather than as an empty
  collection. A genuinely empty collection still reads as empty.
- Subsonic error codes are translated into readable messages.
- Bandcamp's unsupported endpoints (`search3`, `getPlaylist`) answer with HTTP
  200 and a body that carries no data. They are now reported as explicit errors
  so it is visible whether the server rejected the call or returned nothing.
- The Bandcamp connection is verified at startup and warns on failure.
- mpv is driven over its JSON IPC instead of the process command line.
- `Space` toggles pause/resume and `x` stops playback. The now-playing bar
  shows a `⏸ paused` indicator.
- Each track is loaded over IPC with its own title. A command-line
  `force-media-title` is global in mpv and applied one title to the whole queue,
  so playlists and the jukebox showed the wrong track name.
- Cover art no longer loses bright areas. The character ramp was not monotonic —
  ink rose to `@` and then fell again, which rendered near-white artwork almost
  blank and made ordinary covers look noisy.
- Cover art rows are padded to a uniform width, so the right edge inside the
  panel stays straight.

### Changed

- Cover art renders on a monotonic five-step block ramp, so covers keep their
  tonal gradation. It looks different from previous releases; `ascii` and
  `halfblock` are available via `TERMINALDROME_COVER`.
- Responses that were previously reported as an empty collection are now
  reported as errors.
- Credential handling moved from `src/credentials.rs` into `src/config.rs`.
  This is internal; the configuration file format is unchanged and existing
  files keep working.
- The ASCII wordmark has a single source in `docs/logo.txt`, shared by the
  startup splash and the README, so the two cannot drift apart.

## [0.9.0] — 2026-09-22

Published to crates.io. Tag `v0.9.0`, commit `3e5b8d5`.

Release notes for this version and the earlier published ones (`0.8.6`,
`0.8.5`, `0.7.4`, `0.7.3`, `0.7.2`, `0.7.1`, `0.7.0`, `0.6.0`, `0.5.1`,
`0.3.4`, `0.3.3`) were not recorded when they were published and are not
reconstructed here — the repository only started tagging releases at 0.9.0, so
there is no reliable source to derive them from. They are left out rather than
guessed.
