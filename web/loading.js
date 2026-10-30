export function formatBudget(used,limit,unit){
 return unit==='bytes'
  ? (used/1048576).toFixed(1)+' / '+(limit/1048576).toFixed(1)+' MiB'
  : used.toLocaleString('en-US')+' / '+limit.toLocaleString('en-US');
}

export function createLoading(){
 const root=document.querySelector('#loading-details'),heading=document.querySelector('#loading-phase');
 const bar=document.querySelector('#loading-progress'),count=document.querySelector('#loading-count');
 const elapsed=document.querySelector('#loading-elapsed'),budgets=document.querySelector('#loading-budgets');
 const note=document.querySelector('#loading-budget-note'),rows=new Map(),budgetValues=new Map();
 let active=false,timer,started,lastChange,lastPaint=0,phase='',done=0,total=0,rendering=false,world=false,runtime=false;
 const seconds=ms=>Math.floor(ms/60000)+'m '+Math.floor(ms/1000)%60+'s';
 function clock(){const now=performance.now();elapsed.textContent='Elapsed: '+seconds(now-started)+'\nLast change: '+seconds(now-lastChange)+' ago';}
 function paint(){
  heading.textContent=phase;
  if(total>0&&(!runtime||done<total||(rendering&&world))){bar.max=total;bar.value=done;}else bar.removeAttribute('value');
  count.textContent=total>0?'Road index: '+done.toLocaleString('en-US')+' / '+total.toLocaleString('en-US'):'';
  if(runtime)count.textContent+='\nGarage rendering: '+(rendering?'ready':'preparing')+'\nWorld area: '+(world?'ready':'preparing');
  clock();lastPaint=performance.now();
 }
 function stage(text,completed=0,maximum=0){
  if(!active)return;
  const changed=phase!==text||done!==completed||total!==maximum;
  const phaseChanged=phase!==text;
  phase=text;done=completed;total=maximum;
  if(changed)lastChange=performance.now();
  if(phaseChanged||completed===maximum||performance.now()-lastPaint>=250)paint();
 }
 function start(){
  if(active)return;
  active=true;runtime=false;started=lastChange=performance.now();root.hidden=false;
  budgets.hidden=true;note.hidden=true;
  budgetValues.clear();
  for(const node of rows.values())node.textContent='';
  phase='';stage('Loading the game...');timer=setInterval(clock,1000);
 }
 start();
 return {
  stage,
  start,
  progress(roads,maximum,renderReady,worldReady){
   if(!active)return;
   if(rendering!==renderReady||world!==worldReady)lastChange=performance.now();
   runtime=true;rendering=renderReady;world=worldReady;
   stage(roads<maximum?'Indexing the world...':'Preparing the garage and world...',roads,maximum);
  },
  budget(name,used,limit,unit){
   if(!active)return;
   let node=rows.get(name);
   if(!node){const label=document.createElement('dt');label.textContent=name;node=document.createElement('dd');budgets.append(label,node);rows.set(name,node);}
   const value=budgetValues.get(name);
   if(value){value.used=used;value.limit=limit;value.unit=unit;}else budgetValues.set(name,{name,used,limit,unit});
   const text=formatBudget(used,limit,unit);
   if(node.textContent!==text){node.textContent=text;lastChange=performance.now();}
   node.title=used.toLocaleString('en-US')+' '+unit+' / '+limit.toLocaleString('en-US')+' '+unit;
   budgets.hidden=false;note.hidden=false;
  },
  finish(){active=false;clearInterval(timer);root.hidden=true;},
  fail(){if(!active)return;paint();active=false;clearInterval(timer);heading.textContent=phase+' (stopped)';},
  snapshot(){return {phase,done,total,rendering,world,budgets:Array.from(budgetValues.values(),value=>({...value}))};},
 };
}
