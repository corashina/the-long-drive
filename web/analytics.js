// Game events for the hosted site. `functions/api/event.js` stores them in D1.
// Only the hosted site has the endpoint, so other hosts send nothing.
const HEARTBEAT_MS=300000;
// A longer frame gap is a hidden tab or a stall, not a frame.
const MAX_FRAME_MS=1000;

export const hostedSite=hostname=>hostname==='the-long-slop.pages.dev'||hostname.endsWith('.the-long-slop.pages.dev');

// `send` takes the JSON body. `role` returns the multiplayer role.
export function createAnalytics({enabled,send,session,role}){
 let build='',frames=0,playMs=0,worstMs=0,last=null,beat=null,errors=0;
 function event(type,data={}){if(enabled)send(JSON.stringify({session,type,build,data}));}
 function totals(type){
  if(!frames)return;
  event(type,{playSec:Math.round(playMs/1000),fps:Math.round(10000*frames/playMs)/10,worstMs:Math.round(worstMs),role:role()});
  frames=0;playMs=0;worstMs=0;
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
   if(playing&&interval>0&&interval<=MAX_FRAME_MS){frames++;playMs+=interval;if(interval>worstMs)worstMs=interval;}
   if(now-beat>=HEARTBEAT_MS){beat=now;totals('heartbeat');}
  },
  // The first start-up failure. One per page.
  error(message){if(errors++)return;event('error',{message:String(message).slice(0,300)});},
  end(){totals('end');},
 };
}
