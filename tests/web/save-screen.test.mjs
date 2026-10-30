import test from 'node:test';
import assert from 'node:assert/strict';
import {SaveScreenState} from '../../web/save-screen.js';

const entry=name=>({name,date:'2026.10.26. 12:00',seed:1,drivenKm:0,version:'b',thumbnail:null});
function screen(count,flags={}){const s=new SaveScreenState();s.set({saves:Array.from({length:count},(_,i)=>entry('s'+String(i).padStart(2,'0'))),autosave:entry('autosave'),last:null},true);Object.assign(s,{inMenu:false,name:'new'},flags);return s;}

test('paging_scrolls_by_two_and_clamps',()=>{
 const s=screen(13);
 assert.deepEqual(s.page(),{page:1,pages:3,count:13});
 s.scroll(true);assert.equal(s.top,0);
 s.scroll(false);assert.equal(s.top,2);
 s.scrollFast(false);assert.equal(s.top,7);assert.deepEqual(s.page(),{page:3,pages:3,count:13});
 s.jump(true);assert.equal(s.top,0);s.jump(false);assert.equal(s.top,7);
 s.set({saves:[entry('a')],autosave:null,last:null});assert.equal(s.top,0);assert.deepEqual(s.page(),{page:1,pages:1,count:1});
});

test('selection_fills_the_name_and_escape_clears_it_first',()=>{
 const s=screen(3);
 s.select(1);assert.equal(s.selected,1);assert.equal(s.name,'s01');
 s.select(9);assert.equal(s.selected,1);
 assert.equal(s.escape(),null);assert.equal(s.selected,null);
 assert.equal(s.escape(),'back');
 s.select('autosave');assert.equal(s.entry().name,'autosave');
 s.set({saves:[],autosave:null,last:null});assert.equal(s.selected,null);
});

test('yes_no_is_exclusive_and_runs_at_once_in_the_main_menu',()=>{
 const s=screen(3);
 s.select(0);
 assert.equal(s.request('load'),false);assert.equal(s.confirm,'load');
 assert.equal(s.request('delete'),false);assert.equal(s.confirm,'delete');
 assert.equal(s.request('save'),false);assert.equal(s.confirm,'save');
 assert.equal(s.answer(true),'save');assert.equal(s.confirm,null);
 s.name='fresh';assert.equal(s.request('save'),true);assert.equal(s.confirm,null);
 s.request('last');assert.equal(s.answer(false),null);
 s.inMenu=true;
 assert.equal(s.request('load'),true);assert.equal(s.request('last'),true);
 assert.equal(s.request('delete'),false);assert.equal(s.confirm,'delete');
});

test('save_is_refused_in_menu_after_death_on_autosave_sleeping_seated_or_empty_name',()=>{
 assert.equal(screen(1).canSave(),true);
 for(const flags of [{inMenu:true},{dead:true},{sleeping:true},{seated:true},{name:'\u0007'},{name:'autosave'}])assert.equal(screen(1,flags).canSave(),false,JSON.stringify(flags));
 const s=screen(1);s.select('autosave');assert.equal(s.canSave(),false);
});

test('cant_save_text_follows_the_source',()=>{
 assert.deepEqual(screen(0,{sleeping:true}).cantSaveText(),['cantSave','whileSleeping']);
 assert.deepEqual(screen(0,{seated:true}).cantSaveText(),['cantSave','whileSitting']);
 assert.deepEqual(screen(0).cantSaveText(),[]);
 const s=screen(0,{sleeping:true,seated:true}),texts=[];
 for(const r of [0.99,0.99,0.99,0,0,0.5])texts.push(s.cantSaveText(()=>r).join(' '));
 assert.equal(texts[0],'dontsit andsleep');
 assert.equal(texts[1],'keeptrying');
 for(let i=1;i<texts.length;i++)assert.notEqual(texts[i],texts[i-1]);
});
