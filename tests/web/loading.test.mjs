import test from 'node:test';
import assert from 'node:assert/strict';
import {createLoading,formatBudget} from '../../web/loading.js';

test('loading keeps counts and budgets on failure, then resets for the next load',t=>{
 let now=0;t.mock.method(performance,'now',()=>now);
 const nodes=new Map();
 const node=()=>({textContent:'',hidden:false,children:[],value:null,
  removeAttribute(name){if(name==='value')this.value=null;},
  append(...children){this.children.push(...children);}});
 globalThis.document={querySelector(selector){if(!nodes.has(selector))nodes.set(selector,node());return nodes.get(selector);},createElement:node};
 t.after(()=>delete globalThis.document);
 const loading=createLoading();t.after(()=>loading.finish());
 assert.equal(nodes.get('#loading-progress').value,null);
 now=2000;loading.progress(24000,50000,false,false);
 assert.equal(nodes.get('#loading-progress').value,24000);
 assert.match(nodes.get('#loading-count').textContent,/Road index: 24,000 \/ 50,000/);
 assert.match(nodes.get('#loading-count').textContent,/Garage rendering: preparing/);
 loading.budget('Asset GPU',120*1048576,512*1048576,'bytes');
 assert.equal(nodes.get('#loading-budgets').children[1].textContent,'120.0 / 512.0 MiB');
 now=3000;loading.progress(50000,50000,true,false);
 assert.equal(nodes.get('#loading-progress').value,null,'world preparation has no known percentage');
 loading.progress(50001,50000,true,false);
 assert.match(nodes.get('#loading-count').textContent,/Road index: 50,000 \/ 50,000/);
 loading.fail();const stopped=nodes.get('#loading-phase').textContent;
 loading.progress(1,50000,false,false);loading.budget('Asset GPU',0,512*1048576,'bytes');
 assert.equal(nodes.get('#loading-phase').textContent,stopped);
 assert.match(stopped,/stopped/);
 assert.equal(nodes.get('#loading-details').hidden,false);
 assert.equal(nodes.get('#loading-budgets').children[1].textContent,'120.0 / 512.0 MiB');
 loading.finish();assert.equal(nodes.get('#loading-details').hidden,true);
 loading.start();assert.equal(nodes.get('#loading-details').hidden,false);
 assert.equal(nodes.get('#loading-budgets').hidden,true);
 assert.equal(formatBudget(3,2,'jobs'),'3 / 2');
});
