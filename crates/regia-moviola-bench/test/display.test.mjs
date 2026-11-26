// The display's interval that the verdicts use, and what the machine says it is: `npm test`, in Node, on
// synthetic series of frame intervals.
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { displayInterval, FAST_SHARE, MIN_N } from '../scripts/analyze.mjs';
import { virtualOf } from '../scripts/machine.mjs';

/** n frames of `v`, with every k-th one `other`. */
const series = (n, v, k = 0, other = v) => Array.from({ length: n }, (_, i) => (k && i % k === k - 1 ? other : v));

test('frames at 120 Hz after a calibration at 60 Hz give 120 Hz; at the calibrated rate the calibration stands', () => {
  const drr = displayInterval(16.7, series(400, 8.3, 10, 16.7));
  assert.deepEqual([drr.ms, drr.corrected, Math.round(drr.fast_pct)], [8.3, true, 90]);
  const steady = displayInterval(8.3, series(400, 8.3, 10, 16.7));
  assert.deepEqual([steady.ms, steady.corrected], [8.3, false]);
  // 144 Hz under a calibration at 60 Hz
  assert.equal(displayInterval(16.7, series(400, 6.9)).ms, 6.9);
});

test('jitter, a slow renderer and a handful of frames leave the calibration as it is', () => {
  // the macOS runner: most frames on the 17 ms interval, 2 % early by jitter
  assert.deepEqual([displayInterval(17, series(1000, 17, 50, 12)).ms, displayInterval(17, series(1000, 17, 50, 12)).corrected], [17, false]);
  // WSL's llvmpipe: every frame later than the interval, which no frame can raise
  assert.equal(displayInterval(16, series(1000, 23, 3, 21)).ms, 16);
  assert.equal(displayInterval(16.7, series(MIN_N - 1, 8.3)).ms, 16.7);
  assert.equal(displayInterval(16.7, []).ms, 16.7);
});

test('a quarter of the frames at the faster rate is enough, less is not', () => {
  const share = (fast, n = 400) => displayInterval(16.7, Array.from({ length: n }, (_, i) => (i < fast ? 8.3 : 16.7)));
  assert.equal(share(FAST_SHARE * 400).ms, 8.3);
  assert.equal(share(FAST_SHARE * 400 - 1).ms, 16.7);
});

test("systemd-detect-virt's answer counts whatever its exit status: none on bare metal, the hypervisor in a VM", () => {
  assert.equal(virtualOf('none\n'), 'none');
  assert.equal(virtualOf('amazon\n'), 'amazon');
  assert.equal(virtualOf(''), 'unknown');
  assert.equal(virtualOf(null), 'unknown');
});
