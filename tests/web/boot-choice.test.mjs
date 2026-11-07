import test from 'node:test';
import assert from 'node:assert/strict';
import {createBootChoice} from '../../web/boot-choice.js';

function mockDocument(){
 const nodes=new Map();
 const node=()=>({hidden:true,value:'',textContent:'',listeners:{},focus(){},addEventListener(type,listener){this.listeners[type]=listener;}});
 return {nodes,querySelector(selector){if(!nodes.has(selector))nodes.set(selector,node());return nodes.get(selector);}};
}
const click=(document,selector)=>document.nodes.get(selector).listeners.click();
const submit=document=>document.nodes.get('#boot-join').listeners.submit({preventDefault(){}});

test('a game reload skips the card, and a failed join shows the form with the error',async()=>{
 const document=mockDocument();
 const choice=createBootChoice({document,preset:{mode:'join',room:{code:'KQXPTR',name:'Bo',password:'pw'}}});
 assert.deepEqual(await choice.pick(),{mode:'join',room:{code:'KQXPTR',name:'Bo',password:'pw'}});
 assert.equal(document.nodes.get('#boot-choice').hidden,true);
 const next=choice.pick('Wrong password.');
 assert.equal(document.nodes.get('#boot-choice').hidden,false);
 assert.equal(document.nodes.get('#boot-join').hidden,false);
 assert.equal(document.nodes.get('#boot-error').textContent,'Wrong password.');
 assert.equal(document.nodes.get('#boot-room').value,'KQXPTR');
 click(document,'#boot-back');
 assert.equal(document.nodes.get('#boot-modes').hidden,false);
 click(document,'#boot-single');
 assert.deepEqual(await next,{mode:'single'});
 assert.equal(document.nodes.get('#boot-choice').hidden,true);
});

test('an invite link opens the join form, and Join keeps the name',async()=>{
 let now=0;const names=[];
 const document=mockDocument();
 const choice=createBootChoice({document,invite:'ABCDEF',name:'Ana',storeName:name=>names.push(name),now:()=>now});
 const picked=choice.pick();
 assert.equal(document.nodes.get('#boot-join').hidden,false);
 assert.equal(document.nodes.get('#boot-error').hidden,true);
 document.nodes.get('#boot-room').value=' abcdef ';now=2500;submit(document);
 assert.deepEqual(await picked,{mode:'join',room:{code:'ABCDEF',name:'Ana',password:''}});
 assert.deepEqual(names,['Ana']);
 assert.equal(choice.waitedMs,2500);
});
