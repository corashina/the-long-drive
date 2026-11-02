import test from 'node:test';
import assert from 'node:assert/strict';
import {createFramePacer} from '../../web/frame-pacer.js';

// Frames started in `seconds` of display refreshes at `hz`, with the given limit.
function frames(hz,limit,seconds=10){
 const pacer=createFramePacer();let count=0;
 for(let i=0;i<hz*seconds;i++)if(pacer.due(i*1000/hz,limit))count++;
 return count/seconds;
}

test('a_fast_display_averages_the_limit',()=>{
 for(const hz of [60,75,120,144,165,240])assert.ok(Math.abs(frames(hz,60)-Math.min(hz,60))<=0.2,`${hz} Hz`);
 assert.ok(Math.abs(frames(144,30)-30)<=0.2);
 assert.ok(Math.abs(frames(60,30)-30)<=0.2);
});

test('a_slow_display_or_no_limit_starts_every_frame',()=>{
 assert.equal(frames(50,60),50);
 assert.equal(frames(144,0),144);
});

test('a_long_frame_does_not_start_late_frames_back_to_back',()=>{
 const pacer=createFramePacer();
 assert.ok(pacer.due(0,60));
 assert.ok(pacer.due(100,60));
 assert.ok(!pacer.due(106.9,60));
 assert.ok(pacer.due(115,60));
});
