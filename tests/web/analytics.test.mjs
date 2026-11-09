import test from 'node:test';
import assert from 'node:assert/strict';
import {createAnalytics, hostedSite, readyData} from '../../web/analytics.js';
import {onRequestPost} from '../../functions/api/event.js';

test('the heartbeat counts only playing frames', () => {
  const sent = [];
  const analytics = createAnalytics({enabled: true, send: body => sent.push(JSON.parse(body)), session: 's', role: () => 1,
    view: () => ({quality: 'low', scale: 75})});
  analytics.build(11140890);
  analytics.frame(0, false);
  for (let now = 10; now <= 1000; now += 10) analytics.frame(now, true);
  // A slow frame.
  analytics.frame(1050, true);
  // A hidden tab: the gap is not a frame.
  analytics.frame(5000, true);
  analytics.frame(5020, false);
  analytics.frame(300000, false);
  assert.deepEqual(sent, [{session: 's', type: 'heartbeat', build: '11140890', data: {playSec: 1, fps: 96.2, worstMs: 50, slowFrames: 1, quality: 'low', scale: 75, role: 1}}]);
  analytics.end();
  assert.equal(sent.length, 1);
});

test('the largest ready event fits the function limit', () => {
  const phases = Object.fromEntries(['graphics', 'manifest', 'prepare', 'interface', 'generation', 'decodeUpload', 'pipeline', 'other']
    .map(name => [name, {durationMs: 1e9 + 0.5}]));
  const data = readyData({loadMs: 1e9, gpu: 'x'.repeat(500), quality: 'custom', timestamps: false,
    trace: {phases, receivedBytes: 1e12, cache: {hit: 1e6, miss: 1e6}}, device: {hardwareConcurrency: 256, deviceMemory: 0.25}});
  assert.equal(Object.keys(data.phases).length, 7);
  const body = JSON.stringify({session: crypto.randomUUID(), type: 'ready', build: '4294967295', data});
  assert.ok(body.length < 2048, body.length);
});

test('only the hosted site sends events', () => {
  assert.ok(hostedSite('thelongdrive.pages.dev'));
  assert.ok(hostedSite('abc.thelongdrive.pages.dev'));
  assert.ok(!hostedSite('127.0.0.1'));
  assert.ok(!hostedSite('evil-thelongdrive.pages.dev'));
});

test('the function stores a valid event with the country and user agent', async () => {
  const rows = [];
  const env = {EVENTS: {prepare: () => ({bind: (...values) => ({run: async () => rows.push(values)})})}};
  const post = body => onRequestPost({env, request: new Request('https://site/api/event', {method: 'POST', body,
    headers: {'User-Agent': 'UA'}})});
  assert.equal((await post(JSON.stringify({session: 's', type: 'ready', build: 'b', data: {loadMs: 5}}))).status, 204);
  assert.deepEqual(rows[0].slice(1), ['s', null, 'UA', 'ready', 'b', '{"loadMs":5}']);
  assert.equal((await post(JSON.stringify({session: 's', type: 'other', build: 'b'}))).status, 400);
  assert.equal((await post('x'.repeat(3000))).status, 413);
  assert.equal(rows.length, 1);
});
