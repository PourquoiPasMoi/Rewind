# Rewind

A VCR for live streams. Add your Twitch and YouTube channels, leave the PC on, and Rewind
records every stream while it's live and stops when it ends, so you can watch it later.

Built with Rust and Tauri 2. Windows 10/11.

## Download

Get `Rewind_<version>_x64-setup.exe` (installer) or `Rewind.exe` (portable, no install) from the
[latest release](https://github.com/PourquoiPasMoi/Rewind/releases/latest).

On first start, open **Setup** and get the two helper programs:

- **yt-dlp** grabs the stream. One click downloads it (about 24 MB). Slow or unstable internet is fine: it resumes where it stopped.
- **ffmpeg** writes the video file. One click installs it with Windows' own `winget`.

## How it works

- Every channel is checked quietly in the background (every 90 seconds by default).
  Twitch checks are about 1 KB; YouTube checks read only the first few KB of the page.
- When a channel goes live, yt-dlp records the stream as it is broadcast, in the quality you
  pick in Setup (Best, 1080p, 720p, 480p or sound only).
- If a stream drops and comes back within 10 minutes, the recording continues on the same tape.
- When the stream ends, the parts are joined into one `.mp4` (no re-encoding, no quality loss).
- Tapes are saved to `Videos\Rewind\<channel>\` with a cover picture and details.
- Closing the window hides Rewind to the tray, where it keeps watching. Quit from the tray
  saves any open recording first. Tapes left unfinished by a crash or power cut are saved
  the next time Rewind starts.

Only record streams you're allowed to keep. Rewind records public live streams; it does not
record DRM-protected services.

## Build from source

Requires Rust (MSVC toolchain) and Node.js.

```powershell
npm install
npx tauri dev
```

Release build:

```powershell
npx tauri build
```
