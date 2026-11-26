# The moviola with a screen reader

The proofs read what the engine gives a screen reader, not what a person hears. `prove/access.js` reads Chrome's accessibility tree on every moment of x100 (`npm run prove`); `prove/uia.mjs` reads Windows' tree of UI Automation in a WebView2, by hand (README.md). Neither one listens. This guide is for that: a walk with a real screen reader, the same on each system, with what it should say at each step.

The app draws its pages in a different engine on each system, so each system needs its own walk:

| System | Engine of the app | Screen readers | Who |
|---|---|---|---|
| Windows | WebView2 (Chromium) | Narrator, NVDA | the owner |
| macOS | WKWebView (WebKit) | VoiceOver | a colleague with a Mac |
| Linux | WebKitGTK (WebKit) | Orca | a colleague with a Linux desktop |

## Before the walk

1. Build and serve the harness, which mounts the app's own screens on synthetic stories (README.md, «The harness and the proofs»):

   ```sh
   cd app/ui
   npm ci --ignore-scripts
   npm run harness
   node prove/serve.js     # prints the addresses; Ctrl+C stops it
   ```

2. Open the first address in a browser of the app's engine: Edge on Windows, Safari on macOS, GNOME Web (Epiphany) on Linux. The app itself works too, on a dossier with a moviola.
3. Start the screen reader: Narrator with Win+Ctrl+Enter, NVDA with Ctrl+Alt+N, VoiceOver with Cmd+F5, Orca with Super+Alt+S.
4. The stage takes arrow keys only in the reader's focus mode. NVDA moves there by itself on a list, otherwise NVDA+Space. In Narrator, Caps Lock+Space turns scan mode off. In VoiceOver, Quick Nav must be off (Left and Right arrows together). Orca follows the focus on a list.

## The walk

Each step names what should be said. The exact words vary by reader, and so does the order of name, role and place. What counts is that the meaning is there, said once.

**alpha** (`#/moviola/alpha?data=demo&lang=en`, 7 moments)

1. Tab until the stage: «The project's lanes: drag to move them, scroll to zoom, the arrow keys go from moment to moment», a list.
2. Home: the first moment, «Milestone, First synthetic step, Oct 14, …, main», its place (1 of 7) and that it is selected.
3. Right arrow, three times: each moment in time order, each said once. The line under the stage and the panel at the playhead stay silent while the stage has the focus.
4. End, then Home: the last moment, then the first.
5. Escape: the choice goes.
6. Tab: «Previous moment», a button. Enter: the moment before is chosen, and the line under the stage says it once.
7. With no moment chosen, the panel beside the stage lists the directions. Each card reads in words: «Main line, active, principale, 4 moments, …», not glued together.
8. «The story as text», under the panel at the playhead, is a disclosure: Enter opens it. Then browse it as a document: a heading «In time order» with a list of 7, then a heading «Direction: …» for each direction, with its state, why it opened, and its moments.

**x100** (`#/moviola/x100?data=demo&lang=en`, 2,700 moments)

9. Tab to the stage, Home, then hold the Right arrow for a few seconds: one moment after another, «… 251 of 2700 …» and on. Nothing stops at 200.
10. PageDown, then PageUp: ten moments on, then ten back.
11. Press minus until the stage draws groups: «Zoomed out: the moments are drawn in groups by lane, and the list goes group by group.»
12. Home, then Right: entering a group of several, its name once, then the moment and its place in the group. At 1280 pixels wide: «Group 1 of 400, 9 moments, main, Jul 1, … to Jul 5, …, Milestone: 5, …, 2 at risk», then «…, 2 of 9». The numbers follow the window's width.
13. PageDown: the first moment of the next group. End: the last entry.
14. 0: «Zoomed in: each moment is drawn, and the list goes from moment to moment in time.»

**Windows only: a contrast theme**

15. Settings › Accessibility › Contrast themes, any theme. The stage is drawn in the theme's colours: the ground and the marks in its background and text, the choice and the playhead in its highlight. The icons of the transport stay visible. Turn the theme off: the moviola's own colours come back, with no reload.

## What to write down

For each step: what was said, whether the place was said, whether anything was said twice or not at all. Then write a comment on the moviola's accessibility issue in the tracker: the system and its version, the reader and its version, the browser or the app and its version, and for each step «ok» or what was heard instead.

The places to watch:

- WebKit, so VoiceOver and Orca: a list on a canvas that names its choice with `aria-activedescendant` is what Chrome and WebView2 read as the proofs show. If the Right arrow moves the stage and the reader says nothing, that is the finding: write it down with the reader's and the browser's versions.
- Narrator in scan mode reads the page, not the stage: the arrows then move Narrator, not the moment chosen.
- At x100, holding an arrow key can make a reader cut each moment short. That is the reader's choice, as long as the moment where the key is released is said in full.
