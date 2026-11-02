import test from 'node:test';
import assert from 'node:assert/strict';
import {actionIndex,actionName,compileBindings,inputsField,keyName,resetAction,setKey} from '../../web/bindings.js';
import {InputState} from '../../web/input.js';

// Source `InitDefault` keys of some actions.
const defaults=[
 {action:'forward',keys:['KeyW',null,null]},{action:'lookup',keys:['Mouse Y',null,null]},{action:'lookdown',keys:['-Mouse Y',null,null]},
 {action:'lookleft',keys:['-Mouse X',null,null]},{action:'lookright',keys:['Mouse X',null,null]},{action:'scrollup',keys:['Mouse ScrollWheel',null,null]},
 {action:'scrolldown',keys:['-Mouse ScrollWheel',null,null]},{action:'pickup',keys:['KeyF',null,null]},{action:'push',keys:['KeyF',null,null]},
 {action:'inventory1',keys:['Digit1',null,null]},{action:'gas',keys:['KeyW',null,null]},{action:'clutch',keys:['ControlLeft','Numpad0','ControlRight']},
 {action:'gear1',keys:['Digit1','Numpad1',null]},
];
const inputs=defaults.map(b=>b.keys);

test('key_names_follow_the_source_get_name',()=>{
 assert.deepEqual(['KeyW','Digit1','Numpad0','ControlLeft','Mouse0','Mouse ScrollWheel','-Mouse X',null].map(keyName),['W','Alpha1','Keypad0','LeftControl','LMB','Scroll','-Mouse X','None']);
 assert.equal(actionName([null,'KeyY',null]),'Y');
 assert.equal(actionName([null,null,null]),'None');
});

test('stored_inputs_need_the_exported_action_list',()=>{
 const field=inputsField(defaults);
 assert.ok(field.valid(inputs));
 assert.ok(!field.valid(inputs.slice(1)));
 assert.ok(!field.valid(inputs.map(()=>['KeyW',null])));
 assert.ok(!field.valid(inputs.map(()=>[1,null,null])));
});

test('rebinding_moves_the_bits_to_the_new_key',()=>{
 let next=setKey(inputs,10,0,'ArrowUp');
 assert.equal(inputs[10][0],'KeyW');
 let maps=compileBindings(defaults,next);
 assert.equal(maps.bits.KeyW,1);assert.equal(maps.playerBits.KeyW,undefined);assert.equal(maps.playerBits.ArrowUp,256);
 assert.equal(maps.bits.KeyF,256);assert.equal(maps.bits.Digit1,2**26);assert.equal(maps.inventoryBits.Digit1,1);
 next=resetAction(next,defaults,10);
 maps=compileBindings(defaults,next);
 assert.equal(maps.playerBits.KeyW,256);
 const state=new InputState(maps);
 state.code('KeyW',true);
 const frame=state.takeFrame();
 assert.equal(frame.held,1);assert.equal(frame.playerHeld,256);assert.equal(frame.playerPressed,256);
});

test('look_and_scroll_come_from_their_bindings',()=>{
 const state=new InputState(compileBindings(defaults,inputs));
 state.mouse(3,-2);state.scroll(.1);
 let frame=state.takeFrame();
 assert.equal(frame.mouseX,3);assert.equal(frame.mouseY,-2);assert.equal(frame.wheel,.1);assert.equal(frame.driveX,3);
 // Look up on a key: one source axis unit per frame. Mouse Y up still looks up, mouse Y down does not look down.
 const next=setKey(setKey(inputs,1,1,'KeyK'),2,0,null);
 state.setMaps(compileBindings(defaults,next));
 state.code('KeyK',true);state.mouse(0,4);
 frame=state.takeFrame();
 assert.equal(frame.mouseY,-1);assert.equal(frame.driveY,4);
});

test('rebound_keys_persist_and_drive_the_shared_key_rule',async()=>{
 const {createSettings,playerSettings}=await import('../../web/settings.js');
 const items=new Map(),storage=()=>({getItem:key=>items.get(key)??null,setItem:(key,value)=>items.set(key,value)});
 const reference={sliders:{},toggles:{}},fields={inputs:inputsField(defaults)},index=actionIndex(defaults);
 const settings=createSettings(storage,reference,fields);
 assert.equal(JSON.parse(playerSettings(settings.values,index)).inventory_gear_keys,true);
 settings.set('inputs',setKey(settings.values.inputs,index.inventory1,0,'KeyT'));
 assert.equal(JSON.parse(playerSettings(settings.values,index)).inventory_gear_keys,false);
 // A page reload reads the stored keys.
 const reloaded=createSettings(storage,reference,fields).values.inputs;
 assert.equal(reloaded[index.inventory1][0],'KeyT');
 // Scroll Up on a key: one source axis unit per frame.
 const state=new InputState(compileBindings(defaults,setKey(reloaded,index.scrollup,1,'KeyK')));
 state.code('KeyK',true);
 assert.equal(state.takeFrame().wheel,1);
});
