// Game events for the hosted site. `functions/api/event.js` stores them in D1.
// Only the hosted site has the endpoint, so other hosts send nothing.
const HEARTBEAT_MS=300000;
// A longer frame gap is a hidden tab or a stall, not a frame.
const MAX_FRAME_MS=1000;
// A frame above this misses 30 FPS.
const SLOW_MS=1000/30;
// Startup trace stages in the `ready` event, in load order.
const PHASES=['graphics','manifest','prepare','interface','generation','decodeUpload','pipeline'];

export const hostedSite=hostname=>hostname==='thelongdrive.pages.dev'||hostname.endsWith('.thelongdrive.pages.dev');

// The `ready` data. `trace` is the startup trace snapshot. `device` is `navigator`.
export function readyData({loadMs,gpu,quality,timestamps,trace,device}){
 const phases={};
 for(const name of PHASES)if(trace.phases[name])phases[name]=Math.round(trace.phases[name].durationMs);
 return {loadMs:Math.round(loadMs),gpu:gpu.slice(0,120),quality,timestamps,cores:device.hardwareConcurrency??null,memoryGb:device.deviceMemory??null,
  receivedMb:Math.round(trace.receivedBytes/104857.6)/10,cacheHits:trace.cache.hit,cacheMisses:trace.cache.miss,phases};
}

// `send` takes the JSON body. `role` returns the multiplayer role. `view` returns the quality preset and the scene scale percent now drawn.
export function createAnalytics({enabled,send,session,role,view}){
 let build='',frames=0,playMs=0,worstMs=0,slowFrames=0,last=null,beat=null,errors=0;
 function event(type,data={}){if(enabled)send(JSON.stringify({session,type,build,data}));}
 function totals(type){
  if(!frames)return;
  const {quality,scale}=view();
  event(type,{playSec:Math.round(playMs/1000),fps:Math.round(10000*frames/playMs)/10,worstMs:Math.round(worstMs),slowFrames,quality,scale,role:role()});
  frames=0;playMs=0;worstMs=0;slowFrames=0;
 }
 return {
  event,
  // The manifest build ID is a number. The event function accepts only a string.
 build(id){build=String(id);},
  // Each game frame. `playing` is false in menus and pauses.
  frame(now,playing){
   if(beat===null)beat=now;
   const interval=last===null?0:now-last;
   last=playing?now:null;
   if(playing&&interval>0&&interval<=MAX_FRAME_MS){frames++;playMs+=interval;if(interval>worstMs)worstMs=interval;if(interval>SLOW_MS)slowFrames++;}
   if(now-beat>=HEARTBEAT_MS){beat=now;totals('heartbeat');}
  },
  // The first start-up failure. One per page.
  error(message){if(errors++)return;event('error',{message:String(message).slice(0,300)});},
  end(){totals('end');},
 };
}
