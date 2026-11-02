import test from 'node:test';
import assert from 'node:assert/strict';
import {createSaveBridge} from '../../web/save-bridge.js';
import {createSaveSession} from '../../web/save-session.js';
import {MenuState} from '../../web/menu.js';
test('late_prepare_after_cancel_is_discarded',async()=>{
 const host={},runtime=createSaveBridge(host);const session=createSaveSession({runtime,storage:{},drawDistance:()=>2500});
 const pending=session.newGame();const request=JSON.parse(host.gameSaveRequest());assert.deepEqual(request.payload,{draw_distance:2500});session.cancel();
 host.gameSaveResult(request.id,'PrepareNewGame',JSON.stringify(request.id),'');await pending;
 const discard=JSON.parse(host.gameSaveRequest());assert.equal(discard.kind,'Discard');assert.equal(discard.payload,request.id);
 host.gameSaveResult(discard.id,'Discard','','');assert.equal(host.gameHasSaveRequest(),false);
});
test('cancel_queued_commit_discards_before_next_prepare',async()=>{
 const host={},runtime=createSaveBridge(host),states=[];
 const session=createSaveSession({runtime,storage:{},drawDistance:()=>2500,onStatus:s=>states.push([s.state,session.pending])});
 const first=session.newGame(),prepare=JSON.parse(host.gameSaveRequest());
 host.gameSaveResult(prepare.id,prepare.kind,'7','');await Promise.resolve();
 session.cancel();assert.equal(session.pending,false);
 const second=session.newGame();
 const discard=JSON.parse(host.gameSaveRequest());assert.equal(discard.kind,'Discard');assert.equal(discard.payload,7);
 host.gameSaveResult(discard.id,discard.kind,'','');await first;assert.equal(session.pending,true);
 const next=JSON.parse(host.gameSaveRequest());assert.equal(next.kind,'PrepareNewGame');
 host.gameSaveResult(next.id,next.kind,'8','');await Promise.resolve();
 const commit=JSON.parse(host.gameSaveRequest());assert.equal(commit.kind,'Commit');assert.equal(commit.payload,8);
 host.gameSaveResult(commit.id,commit.kind,'','');await second;
 assert.deepEqual(states,[['pending',true],['canceled',false],['pending',true],['success',false]]);
 assert.equal(host.gameHasSaveRequest(),false);
});
test('cancel_after_synchronous_dispatch_keeps_pending_and_completion',async()=>{
 const host={},runtime=createSaveBridge(host),states=[];
 const session=createSaveSession({runtime,storage:{},drawDistance:()=>2500,onStatus:s=>states.push([s.state,session.pending])});
 const operation=session.newGame(),prepare=JSON.parse(host.gameSaveRequest());
 host.gameSaveResult(prepare.id,prepare.kind,'7','');await Promise.resolve();
 const commit=JSON.parse(host.gameSaveRequest());assert.equal(commit.kind,'Commit');
 session.cancel();assert.equal(session.pending,true);
 await assert.rejects(session.newGame(),/pending/);
 host.gameSaveResult(commit.id,commit.kind,'','');await operation;
 assert.deepEqual(states,[['pending',true],['success',false]]);assert.equal(host.gameHasSaveRequest(),false);
});
test('capture_sends_autosave_flag_and_returns_summary',async()=>{
 const host={},runtime=createSaveBridge(host);const pending=runtime.capture(true);const request=JSON.parse(host.gameSaveRequest());
 assert.equal(request.kind,'Capture');assert.equal(request.payload.autosave,true);assert.equal(typeof request.payload.date,'string');
 const result={save:{schema_version:20},summary:{seed:3,driven_km:1.5,build_id:7}};host.gameSaveResult(request.id,'Capture',JSON.stringify(result),'');assert.deepEqual(await pending,result);
});
test('new_game_carries_draw_distance',()=>{
 const host={},runtime=createSaveBridge(host);runtime.prepareNewGame(undefined,9000).catch(()=>{});
 assert.deepEqual(JSON.parse(host.gameSaveRequest()).payload,{draw_distance:9000});
});
