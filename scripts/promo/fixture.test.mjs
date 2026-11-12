import { test } from 'node:test';
import assert from 'node:assert/strict';
import { startFixture } from './fixture.mjs';

test('promo server uses coherent synthetic data and confines static roots', async () => {
 const fixture=await startFixture();
 try {
  const json=async path=>(await fetch(fixture.address+path)).json();
  assert.equal((await json('/api/health')).service,'localvox-light');
  const sessions=await json('/api/sessions');
  assert.equal(sessions.length,8);
  const progress=await json(`/api/sessions/${sessions[1].name}/progress`);
  assert.equal(progress.execution.phase,'summary');
  assert.ok(Number.isFinite(Date.parse(progress.execution.since)));
  for(const path of ['/..%2f..%2f.env','/xterm/..%2f..%2f..%2f..%2f.env']) {
   assert.equal((await fetch(fixture.address+path)).status,404);
  }
  assert.match(await (await fetch(fixture.address+'/terminal')).text(),/демонстрационные данные/);
 } finally {await fixture.close();}
});
