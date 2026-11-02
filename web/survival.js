import {applyLayout,rgba} from './ui-layout.js';
import {actionName} from './bindings.js';
// Source `ls.w` words of the sleep labels.
const SLEEP_WORDS={'can-sleep':'fallingasleep','cannot-sleep':'cantsleepyet'};
// `settings` is the settings store: `BAlwaysShow*`, `BHelp` and `inputs`. `language` is the word table (`createLanguage`).
// `actions` is the action index by source name (`actionIndex`).
export function createSurvival(ui,cooldownSeconds,settings,language,actions) {
 const root=document.querySelector('#survival'),death=document.querySelector('#death-shade'),bars=[];
 const sleepRoot=document.createElement('div');sleepRoot.id='sleep-feedback';sleepRoot.hidden=true;root.parentNode.append(sleepRoot);
 const hint=document.createElement('div');hint.id='sleep-hint';hint.className='sleep-label';hint.hidden=true;root.parentNode.append(hint);
 // Source TextV: the bound sleep key and the word.
 const hintText=()=>{hint.textContent=' ['+actionName(settings.values.inputs[actions.sleep])+']: '+language.word('sleep');};
 hintText();settings.listen(hintText);language.listen(hintText);
 const sleepNodes={};let sleepVisible=false,hintVisible=false,visible=false;
 // The HUD updates each frame. Write to the DOM only on change, so the browser does not recompute styles.
 const written=new Map();
 function style(node,key,value){let last=written.get(node);if(!last)written.set(node,last={});if(last[key]!==value){last[key]=value;node.style[key]=value;}}
 function show(node,value){if(node.hidden===value)node.hidden=!value;}
 function clip(direction,amount){const rest=100-Math.max(0,Math.min(1,amount))*100;return direction===0?'inset(0 '+rest+'% 0 0)':direction===1?'inset(0 0 0 '+rest+'%)':direction===2?'inset('+rest+'% 0 0 0)':'inset(0 0 '+rest+'% 0)';}
 for(const role of ['can-sleep','cannot-sleep','need-sleep-background','need-sleep-fill']){
  const node=document.createElement('div');node.className=role.endsWith('sleep')?'sleep-label':'need-part';if(SLEEP_WORDS[role])language.text(node,SLEEP_WORDS[role]);else node.textContent=ui.layouts[role].text;sleepRoot.append(node);sleepNodes[role]=node;
 }
 const sourceFill=ui.layouts['need-sleep-fill'],fillAncestry=sourceFill.ancestry.slice();
 fillAncestry[fillAncestry.length-1]={...fillAncestry.at(-1),anchor_min:[0,0],anchor_max:[1,1]};
 const sleepFill={...sourceFill,ancestry:fillAncestry};
 function resizeSleep(){applyLayout(hint,ui.layouts['sleep-hint'],ui.urls);for(const [role,node] of Object.entries(sleepNodes)){const layout=role==='need-sleep-fill'?sleepFill:ui.layouts[role];applyLayout(node,layout,ui.urls);if(node.className==='need-part')node.style.backgroundColor=rgba(layout.image_color);}}
 function updateSleep(output,hudEnabled){
  sleepVisible=hudEnabled&&output.held&&!output.dead&&output.phase!==2;show(sleepRoot,visible&&sleepVisible);
  // Source `GV`: the sleep hint is help text.
  hintVisible=hudEnabled&&output.showHint&&settings.values.BHelp;show(hint,visible&&hintVisible);
  show(sleepNodes['can-sleep'],output.eligible);show(sleepNodes['cannot-sleep'],!output.eligible);
  const value=output.eligible?output.charge:Math.max(0,Math.min(1,output.cooldownRemaining/cooldownSeconds));
  const slider=ui.need_sliders.sleep;
  style(sleepNodes['need-sleep-fill'],'clipPath',clip(slider.direction,(value-slider.minimum)/(slider.maximum-slider.minimum)));
 }

 const ALWAYS={hp:'BAlwaysShowHP',food:'BAlwaysShowFood',water:'BAlwaysShowWater',bowel:'BAlwaysShowShit',bladder:'BAlwaysShowPiss'};
 for(const role of ['hp','food','water','bowel','bladder']){
  const fill=ui.layouts['need-'+role+'-fill'];
  // Unity Slider expands its zero-value fill into the authored fill area.
  const nodes={role,always:ALWAYS[role],direction:ui.need_sliders[role].direction,fillLayout:{...fill,ancestry:fill.ancestry.slice(0,-1)}};
  for(const part of ['background','fill','icon']){
   const node=document.createElement('div');node.className='need-part';node.setAttribute('aria-hidden','true');root.append(node);nodes[part]=node;
  }
  const meter=document.createElement('meter');meter.className='need-value';meter.min=ui.need_sliders[role].minimum;meter.max=ui.need_sliders[role].maximum;meter.setAttribute('aria-label',role);root.append(meter);nodes.meter=meter;bars.push(nodes);
 }
 function resize(){resizeSleep();for(const b of bars)for(const part of ['background','fill','icon']){const role='need-'+b.role+'-'+part;const layout=part==='fill'?b.fillLayout:ui.layouts[role];applyLayout(b[part],layout,ui.urls);b[part].style.backgroundColor=rgba(layout.image_color);if(ui.layoutUrls[role]){b[part].style.backgroundImage='url("'+ui.layoutUrls[role]+'")';b[part].style.backgroundColor='transparent';}}}
 window.addEventListener('resize',resize);resize();
 return {updateSleep,update(output){const opacity=String(output.opacity);style(death,'opacity',String(output.dead?1-output.lookScale/.8:0));for(const b of bars){
  // Source `BAlwaysShow*`: the bar keeps full alpha when the needs fade.
  const shown=settings.values[b.always]?'1':opacity;style(b.background,'opacity',shown);style(b.fill,'opacity',shown);style(b.icon,'opacity',shown);const value=output[b.role];if(b.meter.value!==value)b.meter.value=value;style(b.fill,'clipPath',clip(b.direction,(value-b.meter.min)/(b.meter.max-b.meter.min)));}},setVisible(value){visible=value;show(root,value);show(death,value);show(sleepRoot,value&&sleepVisible);show(hint,value&&hintVisible);},
  // Survival off hides the food and water bars, as `mainscript` does with `GSettings.survival`.
  setSurvival(value){for(const b of bars)if(b.role==='food'||b.role==='water')for(const part of ['background','fill','icon','meter'])show(b[part],value);}};
}
