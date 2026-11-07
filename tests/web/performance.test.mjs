import test from 'node:test';
import assert from 'node:assert/strict';
import {createPerformance} from '../../web/performance.js';

test('a frame call with the five Rust values fills every CPU stage',t=>{
 let now=0;t.mock.method(performance,'now',()=>now);
 const node=()=>({hidden:true,textContent:''});
 const nodes={'#performance':node(),'#performance-values':node()};
 globalThis.document={hidden:false,querySelector:selector=>nodes[selector],addEventListener(){},removeEventListener(){}};
 globalThis.window={addEventListener(){}};
 t.after(()=>{delete globalThis.document;delete globalThis.window;});
 const panel=createPerformance(()=>0,()=>null,()=>{},()=>0);
 const frame=panel.frame;
 now=10;frame(1,2,3,4,5);
 now=20;frame(1,2,3,4,5);
 const s=panel.snapshot();
 assert.deepEqual([s.physicsMs,s.rapierMs,s.worldMs,s.updateMs,s.renderMs],[1,2,3,4,5]);
});
