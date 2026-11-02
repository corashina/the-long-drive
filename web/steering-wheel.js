// "Steering wheel" block of the Controls screen: pad axis and button binds, H-shifter and live values.
import {AXES} from './gamepad.js';
// Source `inputscript.axisMove`.
export const AXIS_MOVE=0.25;
const AXIS_NAMES=['Steer','Gas','Brake','Clutch','Handbrake'];
const BUTTON_NAMES=['Clutch','Shift up','Shift down','Reverse','Neutral','Gear 1','Gear 2','Gear 3','Gear 4','Gear 5','Gear 6'];
// Source menu text names.
const LIVE_NAMES=['Steer','Throttle','Brake','Clutch','HandBrake'];
// The first axis more than `AXIS_MOVE` from its rest value, as `{pad,axis}`, or null.
export function movedAxis(axes,rest){
 for(let i=0;i<axes.length;i++)if(Math.abs(axes[i]-rest[i])>AXIS_MOVE)return {pad:Math.floor(i/AXES),axis:i%AXES};
 return null;
}
// The first button pressed that is not in `held`, as `{pad,button}`, or null. `held` takes the current bits.
export function pressedButton(buttons,held){
 for(let p=0;p<buttons.length;p++){
  const fresh=buttons[p]&~held[p];held[p]=buttons[p];
  if(fresh)return {pad:p,button:31-Math.clz32(fresh&-fresh)};
 }
 return null;
}
// `root` is the empty fieldset. `drivingSettings` is from `installDrivingSettings`.
// `wheel` is the exported `driving.steering_wheel`. `gamepad` is from `createGamepad`.
export function installSteeringWheel(root,drivingSettings,wheel,gamepad,requestFrame){
 const doc=root.ownerDocument,{settings,changed}=drivingSettings;
 const el=(tag,props,...children)=>{const node=Object.assign(doc.createElement(tag),props);node.append(...children);return node;};
 const rest=new Float32Array(gamepad.axes.length),held=new Uint32Array(gamepad.buttons.length);
 let waiting=null,scheduled=false;
 const stop=()=>{if(!waiting)return;const {button,paint}=waiting;waiting=null;button.textContent='Bind';paint();};
 function poll(){
  scheduled=false;
  if(!waiting)return;
  if(root.closest('[hidden]')){stop();return;}
  gamepad.poll();
  const found=waiting.axis?movedAxis(gamepad.axes,rest):pressedButton(gamepad.buttons,held);
  if(found){waiting.bind(found);changed();stop();return;}
  scheduled=true;requestFrame(poll);
 }
 // A second click on the same Bind cancels. Another Bind replaces the wait.
 function start(axis,button,status,bind,paint){
  const same=waiting?.button===button;stop();if(same)return;
  gamepad.poll();rest.set(gamepad.axes);held.set(gamepad.buttons);
  waiting={axis,button,bind,paint};button.textContent='Cancel';status.textContent=axis?'Move an axis':'Press a button';
  if(!scheduled){scheduled=true;requestFrame(poll);}
 }
 // The menu closes the Controls screen on Escape. A waiting bind takes Escape first.
 doc.defaultView.addEventListener('keydown',event=>{if(event.code!=='Escape'||!waiting)return;event.preventDefault();event.stopImmediatePropagation();stop();},true);
 const bindRow=(name,axis,get,set)=>{
  const status=el('output'),bind=el('button',{type:'button',textContent:'Bind'}),clear=el('button',{type:'button',textContent:'Clear'});
  const paint=()=>{const bound=get();status.textContent=bound?`Pad ${bound.pad+1} ${axis?'axis':'button'} ${(axis?bound.axis:bound.button)+1}`:'Not bound';};
  bind.addEventListener('click',()=>start(axis,bind,status,set,paint));
  clear.addEventListener('click',()=>{if(waiting?.button===bind)stop();set(null);changed();paint();});
  paint();
  return el('div',{className:'wheel-bind'},el('span',{textContent:name}),status,bind,clear);
 };
 const check=(name,get,set)=>{
  const input=el('input',{type:'checkbox',checked:get()});
  input.addEventListener('change',()=>{set(input.checked);changed();});
  return el('label',{className:'wheel-check'},input,name);
 };
 const slider=(name,range,axis,field)=>{
  const input=el('input',{type:'range',min:String(range.minimum),max:String(range.maximum),step:'any',value:String(axis[field])}),output=el('output',{textContent:axis[field].toFixed(2)});
  input.addEventListener('input',()=>{axis[field]=Math.min(range.maximum,Math.max(range.minimum,Number(input.value)));output.textContent=axis[field].toFixed(2);changed();});
  return el('label',{className:'wheel-slider'},name+' ',input,output);
 };
 root.append(el('legend',{textContent:'Steering wheel'}),check('H-shifter',()=>settings.h_shifter,v=>{settings.h_shifter=v;}));
 settings.axes.forEach((axis,i)=>{
  const range=wheel.axes[i];
  const group=el('div',{className:'wheel-axis'},bindRow(AXIS_NAMES[i],true,()=>axis.bound,v=>{axis.bound=v;}),
   el('div',{className:'wheel-checks'},check('On',()=>axis.enabled,v=>{axis.enabled=v;}),check('Invert',()=>axis.invert,v=>{axis.invert=v;})));
  if(range.middle)group.append(slider('Middle',range.middle,axis,'middle'));
  group.append(slider('Dead zone',range.dead,axis,'dead'),slider('Sensitivity',range.sens,axis,'sens'),slider('Max',range.max,axis,'max'));
  root.append(group);
 });
 root.append(el('h4',{textContent:'Buttons'}),...BUTTON_NAMES.map((name,i)=>bindRow(name,false,()=>settings.buttons[i],v=>{settings.buttons[i]=v;})));
 const live=LIVE_NAMES.map(()=>el('output'));
 root.append(el('div',{className:'wheel-live'},...LIVE_NAMES.flatMap((name,i)=>[el('span',{textContent:name+':'}),live[i]])));
 const show=(output,value)=>{const text=value.toFixed(2);if(output.textContent!==text)output.textContent=text;};
 // Rust steer is positive left. Source menu steer is positive right.
 return {live(steer,throttle,brake,clutch,handbrake){show(live[0],-steer);show(live[1],throttle);show(live[2],brake);show(live[3],clutch);show(live[4],handbrake);}};
}
