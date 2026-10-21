# Privacy

Quirewise processes documents on your computer. It collects no data.

- No accounts, no telemetry, no analytics, no crash reports sent anywhere.
- No network access by default. Network use needs your explicit consent in the flow that needs it: converting a web page to PDF, downloading an optional component or model, requesting a signature timestamp, checking certificate revocation, checking for updates, or connecting to a local AI endpoint on this computer.
- Each consented network action is recorded in the local log on your computer.
- Passwords and signing keys stay in memory and are cleared when no longer needed. They are never written to settings or logs.
- Saved visual signatures are encrypted with Windows DPAPI and stay on this computer.
- Logs, cache, recent files and crash reports are stored under `%LOCALAPPDATA%\Quirewise`. Logs never contain document content or passwords. File paths are hashed at the default log level.
- Temporary files live in `%TEMP%\Quirewise` and are deleted when the app exits and on the next start.

You can clear recent files and remove downloaded components from Settings at any time.
