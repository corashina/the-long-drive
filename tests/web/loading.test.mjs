import test from 'node:test';
import assert from 'node:assert/strict';
import {createLoading,formatBudget} from '../../web/loading.js';

function mockDocument(t){
 const nodes=new Map();
 const node=()=>({textContent:'',className:'',hidden:false,children:[],dataset:{},value:null,
  append(...children){this.children.push(...children);},replaceChildren(...children){this.children=children;}});
 globalThis.document={querySelector(selector){if(!nodes.has(selector))nodes.set(selector,node());return nodes.get(selector);},createElement:node};
 t.after(()=>delete globalThis.document);
 return nodes;
}

test('loading shows a total percent, step counts, and keeps them on failure',t=>{
 let now=0;t.mock.method(performance,'now',()=>now);
 const nodes=mockDocument(t);
 const loading=createLoading();
 assert.equal(nodes.get('#loading-room').hidden,true);t.after(()=>loading.finish());
 const steps=nodes.get('#loading-steps').children;
 const row=index=>({state:steps[index].dataset.state,count:steps[index].children[2].textContent});
 assert.equal(steps.length,9);
 assert.equal(nodes.get('#loading-phase').textContent,'Loading 0%');
 now=1000;loading.step('download',1,1,'304 MiB');loading.step('code',1);loading.step('interface',1);
 assert.equal(nodes.get('#loading-phase').textContent,'Loading 5%');
 assert.deepEqual(row(0),{state:'done',count:'304 MiB'});
 now=2000;loading.progress(1000,2000,0,980,0,0,0,5);
 assert.equal(nodes.get('#loading-progress').value,12,'5 boot + 15 x 1/2 roads');
 assert.deepEqual(row(5),{state:'active',count:'1,000 / 2,000'});
 assert.deepEqual(row(7),{state:'waiting',count:''},'world total is not known yet');
 // The index goes past the start roads. The count stops at the start roads.
 now=3000;loading.progress(2500,2000,490,980,70,140,2,5);
 assert.deepEqual(row(5),{state:'done',count:'2,000 / 2,000'});
 assert.equal(nodes.get('#loading-progress').value,59,'5 + 15 + 17.5 + 17.5 + 4');
 // A larger required set lowers the true count but not the shown percent.
 now=4000;loading.progress(2500,2000,490,980,70,280,2,5);
 assert.equal(nodes.get('#loading-progress').value,59);
 assert.equal(row(7).count,'70 / 280');
 loading.budget('Asset GPU',120*1048576,512*1048576,'bytes');
 assert.equal(nodes.get('#loading-budgets').children[1].textContent,'120.0 / 512.0 MiB');
 assert.equal(nodes.get('#loading-resources').hidden,false);
 loading.fail();
 assert.equal(nodes.get('#loading-phase').textContent,'Loading stopped at 59%');
 loading.progress(0,2000,0,980,0,0,0,5);
 assert.equal(row(5).count,'2,000 / 2,000','counts stay after failure');
 assert.equal(nodes.get('#loading-details').hidden,false);
 assert.equal(loading.snapshot().percent,59);
 loading.finish();assert.equal(nodes.get('#loading-details').hidden,true);
 // New game and Load reload only the world.
 loading.start(true);
 assert.equal(nodes.get('#loading-details').hidden,false);
 assert.equal(nodes.get('#loading-resources').hidden,true);
 assert.deepEqual(steps.map(step=>step.hidden),[true,true,true,true,true,false,false,false,false]);
 now=5000;loading.progress(2000,2000,0,980,0,0,0,5);
 assert.equal(nodes.get('#loading-phase').textContent,'Loading 15%','15 of the 95 world weights');
 assert.equal(formatBudget(3,2,'jobs'),'3 / 2');
});

test('a join shows the room, its players and the join steps',t=>{
 let now=0;t.mock.method(performance,'now',()=>now);
 const nodes=mockDocument(t);
 const loading=createLoading();t.after(()=>loading.finish());
 const steps=nodes.get('#loading-steps').children;
 assert.equal(steps[3].hidden,true);
 loading.join('KQXPTR');
 assert.equal(nodes.get('#loading-room').hidden,false);
 assert.equal(nodes.get('#loading-room-code').textContent,'KQXPTR');
 assert.deepEqual(steps.map(step=>step.hidden),Array(9).fill(false));
 now=1000;loading.step('connect',1);now=2000;loading.step('host',5,10,'50%');
 assert.equal(steps[4].children[2].textContent,'50%');
 assert.equal(steps[4].dataset.state,'active');
 loading.players([{name:'Ana',host:true,you:false},{name:'Bo',host:false,you:true}]);
 const rows=nodes.get('#loading-room-players').children;
 assert.deepEqual(rows.map(item=>[item.textContent,item.className,item.children.map(span=>span.textContent)]),[['Ana','host',[]],['Bo','',['(you)']]]);
 // A new join restarts the join steps. Singleplayer after a failed join hides them.
 loading.join('ABCDEF');
 assert.equal(steps[4].dataset.state,'waiting');
 loading.join('');
 assert.equal(nodes.get('#loading-room').hidden,true);
 assert.equal(steps[3].hidden,true);
 // Later loads of the page do not join.
 loading.join('KQXPTR');loading.finish();loading.start(true);
 assert.equal(nodes.get('#loading-room').hidden,true);
 assert.equal(steps[3].hidden,true);
});
