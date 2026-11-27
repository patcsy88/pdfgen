// Tests for license-check.mjs's SPDX expression handling.
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { allowed } from '../license-check.mjs';

describe('license-check allowed()', () => {
  it('accepts allowed licenses and OR alternatives with an allowed side', () => {
    assert.equal(allowed('MIT'), true);
    assert.equal(allowed('MIT OR Apache-2.0'), true);
    assert.equal(allowed('GPL-3.0 OR MIT'), true);
  });

  it('rejects AND and WITH anywhere, even next to an allowed OR alternative', () => {
    // AND binds tighter than OR in SPDX: this is MIT, or (X and Proprietary). Not reviewable by allowlist.
    assert.equal(allowed('MIT OR X AND Proprietary'), false);
    assert.equal(allowed('Proprietary AND X OR MIT'), false);
    assert.equal(allowed('MIT AND Apache-2.0'), false);
    assert.equal(allowed('Apache-2.0 WITH LLVM-exception'), false);
    assert.equal(allowed('MIT OR GPL-2.0 WITH Classpath-exception-2.0'), false);
  });

  it('rejects unknown, missing and non-string licenses', () => {
    assert.equal(allowed('GPL-3.0'), false);
    assert.equal(allowed(undefined), false);
    assert.equal(allowed({ type: 'MIT' }), false);
    assert.equal(allowed(''), false);
  });
});
