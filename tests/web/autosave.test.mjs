import test from 'node:test';
import assert from 'node:assert/strict';
import {createAutosave} from '../../web/autosave.js';

const on={autosave:true,inCar:true};
test('due_after_five_safe_seconds_and_interval',()=>{
 const a=createAutosave();
 assert.equal(a.tick(116,true,true,false,on),false);
 assert.equal(a.tick(120,true,true,false,on),false);
 assert.equal(a.tick(121,true,true,false,on),true);
});
test('unsafe_frame_resets_safe_time',()=>{
 const a=createAutosave();
 a.tick(200,true,true,false,on);
 assert.equal(a.tick(203,true,false,false,on),false);
 assert.equal(a.tick(204,true,true,false,on),false);
 assert.equal(a.tick(208,true,true,false,on),false);
 assert.equal(a.tick(209,true,true,false,on),true);
 a.tick(210,false,true,false,on);
 assert.equal(a.tick(214,true,true,false,on),false);
});
test('reset_waits_the_interval',()=>{
 const a=createAutosave();
 a.tick(100,true,true,false,on);a.reset(105);
 assert.equal(a.tick(224,true,true,false,on),false);
 assert.equal(a.tick(225,true,true,false,on),true);
});
test('toggles_block',()=>{
 const a=createAutosave();
 a.tick(200,true,true,true,{autosave:true,inCar:false});
 assert.equal(a.tick(210,true,true,true,{autosave:true,inCar:false}),false);
 assert.equal(a.tick(210,true,true,true,on),false);
 assert.equal(a.tick(215,true,true,true,on),true);
 assert.equal(a.tick(230,true,true,false,{autosave:false,inCar:true}),false);
});
