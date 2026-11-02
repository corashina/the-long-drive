// Controls key list: source `settingsscript.UpdControlsScreen` rows of `controlsKey`, and `PressedControl` then `ScanControls`.
import {KEYS,MOUSE_BUTTONS,clearAction,keyName,resetAction,resetInputs,setKey} from './bindings.js';
// Source rows from `gas` on sit one row lower.
const GAP='gas';
// `root` holds the rows. `bindings` is the exported `driving.bindings`. Row names are the source `controls_*` words.
export function createControlsScreen(root,resetAll,settings,language,bindings){
 const doc=root.ownerDocument,cells=[];
 const el=(tag,props,...children)=>{const node=Object.assign(doc.createElement(tag),props);node.append(...children);return node;};
 // The key cell that waits for a key, or null.
 let waiting=null;
 function paint(){
  const inputs=settings.values.inputs;
  for(const c of cells)c.button.textContent=c===waiting?'...':keyName(inputs[c.action][c.key]);
 }
 function finish(code){
  const c=waiting;waiting=null;
  if(code!==null)settings.set('inputs',setKey(settings.values.inputs,c.action,c.key,code));
  paint();
 }
 bindings.forEach(({action},i)=>{
  const label=language.text(el('span',{id:'key-row-'+action}),'controls_'+action);
  const buttons=Array.from({length:KEYS},(_,key)=>{
   const button=el('button',{type:'button',className:'key-cell'}),cell={action:i,key,button};
   button.setAttribute('aria-describedby',label.id);
   button.addEventListener('click',()=>{if(waiting)return;waiting=cell;paint();});
   cells.push(cell);return button;
  });
  const none=language.text(el('button',{type:'button'}),'none');none.setAttribute('aria-describedby',label.id);
  none.addEventListener('click',()=>{if(!waiting)settings.set('inputs',clearAction(settings.values.inputs,i));});
  const reset=language.text(el('button',{type:'button'}),'mreset');reset.setAttribute('aria-describedby',label.id);
  reset.addEventListener('click',()=>{if(!waiting)settings.set('inputs',resetAction(settings.values.inputs,bindings,i));});
  root.append(el('div',{className:'key-row'+(action===GAP?' key-gap':'')},label,...buttons,none,reset));
 });
 resetAll.addEventListener('click',()=>{if(!waiting)settings.set('inputs',resetInputs(bindings));});
 // Escape cancels. Other keys and mouse buttons bind. The capture phase keeps the menu and the game from the event.
 // The click after a bound mouse button does nothing.
 let swallow=false;
 const take=(event,code)=>{if(!waiting)return false;event.preventDefault();event.stopImmediatePropagation();finish(code);return true;};
 const win=doc.defaultView;
 win.addEventListener('keydown',event=>take(event,event.code==='Escape'?null:event.code),true);
 win.addEventListener('mousedown',event=>{if(event.button<=2&&take(event,MOUSE_BUTTONS[event.button]))swallow=event.button===0;},true);
 win.addEventListener('click',event=>{if(swallow){swallow=false;event.preventDefault();event.stopImmediatePropagation();}},true);
 settings.listen(paint);
 paint();
 return {paint,cancel(){if(waiting)finish(null);}};
}
