// F4 weather test panel. It sets the weather with the sandbox `weather` action, in every build.
// `guest` is true for a multiplayer guest. The host owns the weather, so a guest cannot set it.
const REFRESH_MS=1000;

export function createWeatherPanel(simulation,guest){
 const root=document.querySelector('#weather-panel'),select=document.querySelector('#weather-kind'),values=document.querySelector('#weather-values');
 let timer=0,waiting=false;
 const show=([reply])=>{
  if(!reply?.ok){values.textContent=reply?.message??'No reply.';return;}
  const {kind,next,blend,locked}=reply.data;
  values.textContent='Now: '+kind+'\nNext: '+next+'\nBlend: '+blend.toFixed(2)+'\nLocked: '+(locked?'yes':'no');
  if(document.activeElement!==select)select.value=locked?kind:'';
 };
 // Before the start, the worker has no world to ask. The worker answers in order.
 function send(action){
  if(!simulation.started)return;
  waiting=true;
  simulation.sandbox(JSON.stringify([action])).then(JSON.parse).then(show).finally(()=>{waiting=false;});
 }
 // One refresh at a time.
 function refresh(){select.disabled=guest();if(!waiting)send({action:'forecast'});}
 select.addEventListener('change',()=>{
  if(guest())return;
  send({action:'weather',kind:select.value||null});
  select.blur();
 });
 window.addEventListener('keydown',event=>{
  if(event.code!=='F4'||event.repeat||event.ctrlKey||event.altKey||event.metaKey)return;
  event.preventDefault();root.hidden=!root.hidden;
  clearInterval(timer);timer=0;
  if(!root.hidden){refresh();timer=setInterval(refresh,REFRESH_MS);}
 });
}
