// The help's two tables: the keys this version has, and the words the app shows, with what each one means. The
// words are the UX prototype's, with those #439/D31 found missing (memo, chronicler, idle, Project, precheck, a
// session's identifier, the flag that signs as the owner) and the difference between a Decision and a gate.

/** A key's description, and the keys as they are typed. */
export const KEYS: readonly [string, string][] = [
  ["keys-g", "g b · g d · g m · g c · g s"],
  ["keys-help", "?"],
  ["keys-escape", "Esc"],
];

/** Each word's term and its meaning, in the order a newcomer meets them. */
export const GLOSSARY: readonly [string, string][] = [
  ["words-decision-term", "words-decision"],
  ["words-gate-term", "words-gate"],
  ["words-memo-term", "words-memo"],
  ["words-chain-term", "words-chain"],
  ["words-unblocks-term", "words-unblocks"],
  ["words-dailies-term", "words-dailies"],
  ["words-idle-term", "words-idle"],
  ["words-moviola-term", "words-moviola"],
  ["words-chronicler-term", "words-chronicler"],
  ["words-dossier-term", "words-dossier"],
  ["words-project-term", "words-project"],
  ["words-offtracker-term", "words-offtracker"],
  ["words-orchestrator-term", "words-orchestrator"],
  ["words-controller-term", "words-controller"],
  ["words-precheck-term", "words-precheck"],
  ["words-worker-term", "words-worker"],
  ["words-round-term", "words-round"],
  ["words-session-term", "words-session"],
  ["words-lock-term", "words-lock"],
  ["words-behalf-term", "words-behalf"],
  ["words-fallback-term", "words-fallback"],
];

/** The moviola's part (#487): the prototype viewer's guide in the page (`guida.js`, its tour and its glossary),
 * made again as the help's section: the moviola's keys, then the signs on its stage. */
export const MOVIOLA_KEYS: readonly [string, string][] = [
  ["keys-search", "/"],
  ["keys-moments", ", ."],
  ["moviola-keys-arrows", "← →"],
  ["moviola-keys-lanes", "↑ ↓"],
  ["moviola-keys-ends", "Home · End"],
  ["moviola-keys-zoom", "+ · − · 0"],
  ["moviola-keys-enter", "Enter"],
  ["moviola-keys-escape", "Esc"],
];

export const MOVIOLA_WORDS: readonly [string, string][] = [
  ["moviola-word-kinds-term", "moviola-word-kinds"],
  ["moviola-word-health-term", "moviola-word-health"],
  ["moviola-word-rings-term", "moviola-word-rings"],
  ["moviola-word-badges-term", "moviola-word-badges"],
  ["moviola-word-undeclared-term", "moviola-word-undeclared"],
  ["moviola-word-lanes-term", "moviola-word-lanes"],
  ["moviola-word-playhead-term", "moviola-word-playhead"],
  ["moviola-word-joint-term", "moviola-word-joint"],
];
