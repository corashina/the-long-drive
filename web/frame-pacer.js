// Frame limit pacing. Frames start only in animation frames, so they stay in step with the display.
// Each frame owns one slot of 1000/limit ms. A display faster than the limit then averages the limit.
// SLACK_MS lets a frame start a little early: animation frame times move by about 1 ms.
const SLACK_MS=2;
// `due(now,limit)` is true when a frame starts at `now`. A limit of 0 or less starts every frame.
export function createFramePacer(){
 let next=null;
 return {due(now,limit){
  if(!(limit>0)){next=null;return true;}
  if(next!==null&&now<next-SLACK_MS)return false;
  const interval=1000/limit;
  // After a long frame, start new slots from now. Do not run late frames back to back.
  next=next===null||now>next+interval?now+interval:next+interval;
  return true;
 }};
}
// Rust waits for `wake` while a limit is set. `wake` runs in a task after the animation frame:
// a wake inside the frame moves the renderer's own pending animation frame to the next one.
export function installFramePacer(raf,limit,wake){
 const pacer=createFramePacer(),channel=new MessageChannel();
 channel.port1.onmessage=()=>wake();
 const tick=now=>{if(pacer.due(now,limit()))channel.port2.postMessage(0);raf(tick);};
 raf(tick);
}
