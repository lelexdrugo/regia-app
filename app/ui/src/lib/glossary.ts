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
