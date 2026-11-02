import test from 'node:test';
import assert from 'node:assert/strict';
import {talkState} from '../../web/voice.js';

test('source VoiceChatUpdate: push to talk, toggle and continuous',()=>{
 const key=(held,pressed,blocked=false)=>({held,pressed,blocked});
 assert.equal(talkState('pushToTalk',false,key(true,true)),true);
 assert.equal(talkState('pushToTalk',true,key(false,false)),false);
 assert.equal(talkState('pushToTalk',false,key(true,false,true)),false,'no talk while the input is blocked');
 assert.equal(talkState('toggle',false,key(true,true)),true);
 assert.equal(talkState('toggle',true,key(true,false)),true,'a held key does not toggle again');
 assert.equal(talkState('toggle',true,key(true,true)),false);
 assert.equal(talkState('toggle',false,key(true,true,true)),false);
 assert.equal(talkState('continous',false,key(false,false,true)),true);
});
