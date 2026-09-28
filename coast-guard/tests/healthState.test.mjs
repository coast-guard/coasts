import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/components/healthState.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } });
const { portHealth, serviceHealth, isServiceRunning } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

test('Docker startup stays amber even when a port already accepts connections', () => {
  for (const probe of [undefined, false, true]) {
    assert.equal(portHealth(probe, 'running (starting)'), 'starting');
  }
});

test('unhealthy and exited services cannot be green from a stale port probe', () => {
  for (const status of ['running (unhealthy)', 'exited', 'restarting', 'down']) {
    assert.equal(portHealth(true, status), 'unhealthy');
    assert.equal(serviceHealth(status), 'unhealthy');
  }
});

test('healthy Docker status still requires a successful external port probe', () => {
  assert.equal(portHealth(false, 'running (healthy)'), 'unhealthy');
  assert.equal(portHealth(true, 'running (healthy)'), 'healthy');
});

test('unmatched ports and services without healthchecks preserve probe behavior', () => {
  for (const status of [undefined, 'running']) {
    assert.equal(portHealth(undefined, status), 'checking');
    assert.equal(portHealth(false, status), 'unhealthy');
    assert.equal(portHealth(true, status), 'healthy');
  }
});

test('a restart can reenter starting after previously being healthy', () => {
  assert.deepEqual(['running (healthy)', 'running (starting)', 'running (healthy)'].map(serviceHealth),
    ['healthy', 'starting', 'healthy']);
});

test('health status does not disable running-service controls', () => {
  for (const status of ['running', 'running (starting)', 'running (healthy)', 'running (unhealthy)']) {
    assert.equal(isServiceRunning(status), true);
  }
  for (const status of ['exited', 'down', 'restarting']) assert.equal(isServiceRunning(status), false);
});

test('cleared service cache returns to live probe behavior', () => {
  assert.equal(portHealth(true, 'running (starting)'), 'starting');
  assert.equal(portHealth(true, undefined), 'healthy');
});
