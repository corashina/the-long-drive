// F4 weather test panel: a map of the cells near the player, wind, temperature, the lock select and a cell spawn.
// The sandbox `weather` action locks a kind. `spawn_cell` adds a full-strength cell at the player. In every build.
// `guest` is true for a multiplayer guest. The host owns the weather, so a guest cannot set it.
const REFRESH_MS=1000;
// Map radius in meters. The map is x to the right and z down.
const MAP_RANGE=8000;
// Arrow pixels per wind m/s.
const ARROW_SCALE=4;
// Cell colors by kind. The kind list comes from the forecast reply. Other kinds are grey.
const COLORS={clear:'#ffd84a',cloudy:'#c8c8c8',rain:'#3c8cff',thunderstorm:'#b44cff',fog:'#9fb7a8',snow:'#ffffff'};

export function createWeatherPanel(simulation,guest){
 const root=document.querySelector('#weather-panel'),select=document.querySelector('#weather-kind'),values=document.querySelector('#weather-values');
 const spawnKind=document.querySelector('#weather-spawn-kind'),spawn=document.querySelector('#weather-spawn'),map=document.querySelector('#weather-map');
 const context=map.getContext('2d'),half=map.width/2,scale=half/MAP_RANGE;
 let timer=0,waiting=false,kinds=null;
 // Fills both selects one time, from the first reply.
 function addKinds(list){
  kinds=list;
  for(const target of [select,spawnKind])for(const kind of list)target.add(new Option(kind[0].toUpperCase()+kind.slice(1),kind));
 }
 function draw({cells,player,wind}){
  context.clearRect(0,0,map.width,map.height);
  context.strokeStyle='#fff4';
  context.beginPath();context.arc(half,half,half-1,0,Math.PI*2);context.stroke();
  for(const cell of cells){
   const x=half+(cell.center[0]-player[0])*scale,y=half+(cell.center[1]-player[1])*scale,r=cell.radius*scale;
   if(Math.hypot(x-half,y-half)-r>half)continue;
   context.globalAlpha=0.15+0.5*cell.strength;
   context.fillStyle=COLORS[cell.kind]??'#888';
   context.beginPath();context.arc(x,y,r,0,Math.PI*2);context.fill();
  }
  context.globalAlpha=1;
  const length=wind.speed*wind.gust*ARROW_SCALE,ex=half+wind.dir[0]*length,ey=half+wind.dir[1]*length,angle=Math.atan2(wind.dir[1],wind.dir[0]);
  context.strokeStyle='#fff';context.lineWidth=2;
  context.beginPath();context.moveTo(half,half);context.lineTo(ex,ey);
  context.moveTo(ex,ey);context.lineTo(ex-6*Math.cos(angle-0.5),ey-6*Math.sin(angle-0.5));
  context.moveTo(ex,ey);context.lineTo(ex-6*Math.cos(angle+0.5),ey-6*Math.sin(angle+0.5));
  context.stroke();context.lineWidth=1;
  context.fillStyle='#f33';
  context.beginPath();context.arc(half,half,3,0,Math.PI*2);context.fill();
 }
 const show=([reply])=>{
  if(!reply?.ok){values.textContent=reply?.message??'No reply.';return;}
  const data=reply.data;
  if(!kinds)addKinds(data.kinds);
  const {kind,rain,snow,snow_cover,locked,cells,wind,temperature,wetness}=data;
  values.textContent='Here: '+kind+'\nRain: '+rain.toFixed(2)+'  Wet: '+wetness.toFixed(2)
   +'\nSnow: '+snow.toFixed(2)+'  Cover: '+snow_cover.toFixed(2)
   +'\nWind: '+wind.speed.toFixed(1)+' m/s x'+wind.gust.toFixed(2)+'\nTemperature: '+temperature.toFixed(1)+' °C'
   +'\nCells: '+cells.length+'\nLocked: '+(locked?'yes':'no');
  if(document.activeElement!==select)select.value=locked?kind:'';
  draw(data);
 };
 // Before the start, the worker has no world to ask. The worker answers in order.
 function send(action){
  if(!simulation.started)return;
  waiting=true;
  simulation.sandbox(JSON.stringify([action])).then(JSON.parse).then(show).finally(()=>{waiting=false;});
 }
 // One refresh at a time.
 function refresh(){
  select.disabled=spawnKind.disabled=spawn.disabled=guest();
  if(!waiting)send({action:'forecast'});
 }
 select.addEventListener('change',()=>{
  if(guest())return;
  send({action:'weather',kind:select.value||null});
  select.blur();
 });
 spawn.addEventListener('click',()=>{
  if(guest()||!spawnKind.value)return;
  send({action:'spawn_cell',kind:spawnKind.value});
  spawn.blur();
 });
 window.addEventListener('keydown',event=>{
  if(event.code!=='F4'||event.repeat||event.ctrlKey||event.altKey||event.metaKey)return;
  event.preventDefault();root.hidden=!root.hidden;
  clearInterval(timer);timer=0;
  if(!root.hidden){refresh();timer=setInterval(refresh,REFRESH_MS);}
 });
}
