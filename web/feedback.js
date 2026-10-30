import {applyLayout} from './ui-layout.js';
import {boundNames} from './bindings.js';
import {HELP,helpText} from './help-text.js';
export function formatGear(gear){return gear<0?'R':gear===0?'N':String(gear);}
/** Item prompt bits (Rust `ItemPrompt`). */
export const PROMPT={ATTACH:1,COIL:2,DRAIN:4,RELOAD:8,UNLOAD:16,LOOK:32,FOCUS:64};
/** Computer prompt bits (Rust `ComputerPrompt`). TYPING routes the keyboard to the computer. */
export const COMPUTER={TURN_ON:1,TURN_OFF:2,USE:4,CONNECT:8,DISCONNECT:16,TYPING:32};
export const ACTION={USE:1,DOOR:2,PICKUP:4,DROP:8,MOUNT:16,DISMOUNT:32,FILL:64,TUNE:128,ENTER:256,EXIT:512,SWITCH:1024,PUSH:2048,GRIP:4096,THROW:8192,ROTATE:32768,RELEASE_GRIP:65536,PRIMARY_SWITCH:131072,SCROLL:262144,CONSUME:524288,DRINK:1048576,HELD_SWITCH:2097152,WASH:4194304,SCRUB:8388608,EQUIP:16777216,RECENTER:33554432,RELEASE_EQUIPMENT:67108864,RELEASE_EQUIPMENT_SEATED:134217728,TAKE_ONE:268435456,SMOKE:536870912,PUT_ON:1073741824,CRANK:2147483648};
// Source `mainscript.fluids[type].lname` words.
const FLUIDS=['gas','oil','alcohol','water','blood','diesel'];
// `word` returns a source `ls.w` word by name.
export function formatRadio(r,word){if(r.context===11&&Number.isFinite(r.radioVolume))return word('volume')+': '+Math.round(r.radioVolume*10).toString().padStart(2,'0');if(r.context===12&&Number.isFinite(r.radioFrequency))return (r.radioAm?word('am')+' '+r.radioFrequency.toFixed(0).padStart(3,'0'):word('fm')+' '+r.radioFrequency.toFixed(1).padStart(4,'0'));return '';}
function volume(value){return (Math.round(value*10)/10).toFixed(1).replace(/\.0$/,'');}
// Source `GetCapString`. `index` returns the word of an `ls.w` index.
export function formatTank(r,word,index) {
 if(r.noText)return '';
 if(r.context===14)return Math.round(r.amount).toString();
 const prefix=r.hasName?index(r.fluidWord)+':\n':'';
 if(r.amount<=Math.fround(.05))return prefix+word('empty');
 const amount=volume(r.amount),capacity=volume(r.capacity);
 let result=prefix+(amount===capacity?amount+' '+word('liter')+' '+word('full'):amount+' / '+capacity+' '+word('liter'));
 // Source GetCapString follows the tank's fluid list order: type + 1 per 4 bits, 0 ends.
 for(let order=r.fluidOrder??0;order;order>>>=4){const i=(order&15)-1,percent=Math.fround(Math.fround(r.quantities[i]/r.amount)*100);if(percent>=1)result+='\n'+Math.round(percent)+word('percent')+' '+word(FLUIDS[i]);}
 return result;
}
/** Rust `Crosshair` layer bits. E, adjust and F hold a sprite choice. */
export const CROSSHAIR={E_SHIFT:0,ADJUST_SHIFT:2,F_SHIFT:4,MMB:128,LMB:256,DISMOUNT:512,CAN_PICKUP:1024,PICKUP:2048,TOO_HEAVY:4096,DEFAULT:8192,WEAPON:16384,MMB_SPRITE_SHIFT:16};
const E_ICONS=['','enter','exit','switch'],ADJUST_ICONS=['','door','slide','slide-vertical'],F_ICONS=['','push','mount','hose','attach'];
/** Source `mainscript.CrossHairStuff` layers in sibling (draw) order: the icon role or `ui.sprites` index, or null when hidden, and the source size. */
export function crosshairLayers(bits){
 const e=E_ICONS[bits>>CROSSHAIR.E_SHIFT&3],adjust=ADJUST_ICONS[bits>>CROSSHAIR.ADJUST_SHIFT&3],f=F_ICONS[bits>>CROSSHAIR.F_SHIFT&7];
 const on=(bit,role)=>bits&bit?role:null,mmb=bits>>>CROSSHAIR.MMB_SPRITE_SHIFT;
 return [[on(CROSSHAIR.DISMOUNT,'dismount'),75,40],[on(CROSSHAIR.CAN_PICKUP,'pickup'),75,40],[on(CROSSHAIR.PICKUP,'carrying'),75,40],
  [on(CROSSHAIR.DEFAULT,'default'),35,35],[on(CROSSHAIR.TOO_HEAVY,'too-heavy'),75,40],[f||null,40,40],[adjust||null,40,40],
  [on(CROSSHAIR.WEAPON,'weapon'),50,50],[e||null,40,40],[on(CROSSHAIR.MMB,mmb?mmb-1:'cap'),40,40],[on(CROSSHAIR.LMB,'fill'),40,40]];
}
/** Source `GCPickups` alphas under the carrying layer: medium and heavy, by `weightM`. */
export function pickupWeightAlphas(weight){
 if(!(weight<0.9))return [0,0];
 const inverse=(a,b,v)=>Math.max(0,Math.min(1,(v-a)/(b-a)));
 return [inverse(0.9,0.5,weight),inverse(0.5,0,weight)];
}
export class FeedbackState {
 constructor(word=name=>name,index=String){this.word=word;this.index=index;this.record=null;this.help=0;this.visible=false;this.enabled=true;this.active=false;this.fluidText='';this.fluidWord=undefined;this.capacity=undefined;this.amountKey=undefined;this.percentKeys=[-1,-1,-1,-1,-1,-1];this.hasName=undefined;this.noText=undefined;this.empty=undefined;this.lmbUse=false;this.actions=-1;this.context=-1;this.carrying=false;this.notice='';this.throwCharge=0;this.throwActive=false;this.prompts=0;this.computer=0;this.crosshair=0;this.weight=1;}
 // Formats the fluid text again with the current words.
 relabel(){this.amountKey=undefined;if(this.record)this.update(this.record);}
 setVisible(active){this.active=active;this.visible=active&&this.enabled;}
 toggle(){this.enabled=!this.enabled;this.visible=this.active&&this.enabled;}
 update(r){
  this.record=r;
  const amountKey=Math.round(r.amount*(r.context===14?1:10)),empty=r.amount<=Math.fround(.05);
  let changed=r.context!==this.context||amountKey!==this.amountKey||r.capacity!==this.capacity||r.fluidWord!==this.fluidWord||r.hasName!==this.hasName||r.noText!==this.noText||empty!==this.empty||r.fluidOrder!==this.fluidOrder;
  for(let i=0;i<6;i++){const percent=r.amount>0?Math.fround(Math.fround(r.quantities[i]/r.amount)*100):0;const key=percent>=1?Math.round(percent):-1;if(this.percentKeys[i]!==key){changed=true;this.percentKeys[i]=key;}}
  const radioChanged=r.radioAm!==this.radioAm||r.radioFrequency!==this.radioFrequency||r.radioVolume!==this.radioVolume;
  if(changed||radioChanged){this.fluidText=r.context===11||r.context===12?formatRadio(r,this.word):formatTank(r,this.word,this.index);this.amountKey=amountKey;this.empty=empty;this.capacity=r.capacity;this.fluidWord=r.fluidWord;this.hasName=r.hasName;this.noText=r.noText;this.fluidOrder=r.fluidOrder;}
  this.radioAm=r.radioAm;this.radioFrequency=r.radioFrequency;this.radioVolume=r.radioVolume;
  const detailChanged=r.throwCharge!==this.throwCharge||r.throwActive!==this.throwActive;
  this.throwCharge=r.throwCharge;this.throwActive=r.throwActive;
  const actionsChanged=r.actions!==this.actions||r.context!==this.context||r.carrying!==this.carrying||r.notice!==this.notice||r.lmbUse!==this.lmbUse||r.prompts!==this.prompts||r.computer!==this.computer||r.crosshair!==this.crosshair||r.weight!==this.weight||r.help!==this.help;
  this.lmbUse=r.lmbUse;this.prompts=r.prompts;this.computer=r.computer;this.crosshair=r.crosshair;this.weight=r.weight;this.help=r.help;this.actions=r.actions;this.context=r.context;this.carrying=r.carrying;this.notice=r.notice;
  return changed||radioChanged||actionsChanged||detailChanged;
 }
}
/** Source `mainscript.CrosshairColor` tint of the old crosshair, from the layer bits. */
export function oldCrosshairColor(bits,colors){
 if(bits&(3<<CROSSHAIR.E_SHIFT|3<<CROSSHAIR.ADJUST_SHIFT))return colors.e;
 if(bits&(7<<CROSSHAIR.F_SHIFT|CROSSHAIR.CAN_PICKUP|CROSSHAIR.DISMOUNT))return colors.f;
 if(bits&CROSSHAIR.MMB)return colors.mmb;
 return colors.default;
}
const css=([r,g,b,a])=>`rgba(${r*255},${g*255},${b*255},${a})`;
// `settings` is the settings store: `BHelp`, `BCrosshair`, `BOldCrosshair` and `FCrosshairScale`.
// `actions` is the action index by source name (`actionIndex`). `language` is the word table (`createLanguage`).
export function createFeedback(ui,manifest,settings,actions,language) {
 const state=new FeedbackState(language.word,language.index),root=document.querySelector('#hud'),icon=document.querySelector('#crosshair'),fluid=document.querySelector('#fluid-text'),carry=document.querySelector('#carry-help'),helpNode=document.querySelector('#help-text'),notice=document.querySelector('#action-notice'),charge=document.querySelector('#throw-charge'),level=document.querySelector('#throw-level');
 const gear=document.querySelector('#driving-gear');let lastGear,wasSeated;
 const rows=[
  [ACTION.USE,'use','{use}: <openclose>'],
  [ACTION.DOOR,'door','{lmb}+[<mouse>]: <openclose>'],
  [ACTION.PICKUP,'pickup','{pickup}: <pickup>'],
  [ACTION.DROP,'drop','{pickup}: Drop'],
  [ACTION.MOUNT,'mount','{mountdismount}: <mount>'],
  [ACTION.DISMOUNT,'dismount','{mountdismount}: <dismount>'],
  [ACTION.FILL,'fill','{lmb}: <fill>'],
  [ACTION.CONSUME,'eat','{lmb}: <eat>'],
  [ACTION.SMOKE,'smoke','{lmb}: <eatsmoke>'],
  [ACTION.TAKE_ONE,'takeOne','{use}: <takeone>'],
  [ACTION.PUT_ON,'putOn','Tap {equiprecenter}: <puton>'],
  [ACTION.DRINK,'drink','Hold {lmb}: <drink>'],
  [ACTION.TUNE,'tune','{use}+[<mouse>] / {scrollup}: <openclose>'],
  [ACTION.ENTER,'enter','Release {use}: <sit>'],
  [ACTION.EXIT,'exit','{use}: <getoutstand>'],
  [ACTION.SWITCH,'switch','{use}: <turnSwitch>'],
  [ACTION.PRIMARY_SWITCH,'primary','{use} / {lmb}: <turnSwitch>'],
  [ACTION.PUSH,'push','{push}: <push>'],
  [ACTION.GRIP,'grip','{push}: Grip'],
  [ACTION.RELEASE_GRIP,'release','{forward}{left}{back}{right}: <push> / {push}: Release'],
  [ACTION.THROW,'throw','{pickup}: Drop / hold to throw'],
  [ACTION.SCROLL,'scroll','{scrollup}: <openclose>'],
  [ACTION.HELD_SWITCH,'heldSwitch','{lmb}: Switch flashlight'],
  [ACTION.WASH,'wash','{lmb}: <wash>'],
  [ACTION.SCRUB,'scrub','{lmb}: <scrub>'],
  [ACTION.EQUIP,'equip','Tap {equiprecenter}: <equip>'],
  [ACTION.RECENTER,'recenter','Hold {equiprecenter}: recenter'],
  [ACTION.RELEASE_EQUIPMENT,'releaseEquipment','{equiprecenter} or {mmb}: release'],
  [ACTION.RELEASE_EQUIPMENT_SEATED,'releaseEquipmentSeated','{mmb}: release'],
  [ACTION.CRANK,'crank','{use}+[<mouse>]: <crank>'],
  [0,'hoseAttach','{mountdismount}: <stickin>',PROMPT.ATTACH],
  [0,'coilUp','{reload}: <coilup>',PROMPT.COIL],
  [0,'drain','{lmb}: <drain>',PROMPT.DRAIN],
  [0,'reload','{reload}: <reload>',PROMPT.RELOAD],
  [0,'unload','{use}: <unload>',PROMPT.UNLOAD],
  [0,'binocularsUse','{lmb}: <use>',PROMPT.LOOK],
  [0,'binocularsFocus','{mmb}: <focus>',PROMPT.FOCUS],
  [0,'computerTurnOn','{reload}: <turnon>',0,COMPUTER.TURN_ON],
  [0,'computerTurnOff','{reload}: <turnoff>',0,COMPUTER.TURN_OFF],
  [0,'computerUse','{lmb}: <use>',0,COMPUTER.USE],
  [0,'computerConnect','{use}: <connect>',0,COMPUTER.CONNECT],
  [0,'computerDisconnect','{use}: <disconnect>',0,COMPUTER.DISCONNECT]
 ].map(([bit,role,template,prompt=0,computer=0])=>{const n=document.createElement('p');n.className='action-row';n.hidden=true;root.append(n);return {bit,prompt,computer,role,template,n,family:0,top:0,line:0,offset:-1};});
 const named=Object.fromEntries(rows.map(r=>[r.role,r.n]));
 // fpscontroller GCWeapon: the gun crosshair sprite tinted by its color. It is one crosshair layer.
 const weapon=document.createElement('div');weapon.id='weapon-crosshair';weapon.hidden=true;let weaponIcon=0;
 const layers=crosshairLayers(0).map((_,i)=>{if(i===7)return weapon;const n=document.createElement('img');n.alt='';n.hidden=true;return n;});
 icon.append(...layers);
 const pickupChildren=['carrying-medium','carrying-heavy'].map(role=>{const n=document.createElement('img');n.alt='';n.src=ui.urls[ui.icons[role]];n.style.opacity=0;n.hidden=true;return n;});
 layers[2].after(...pickupChildren);
 layers[3].style.opacity=ui.layouts.reticle.image_color[3];
 // Source GC and GC2: one sprite, tinted by the target and by the carried weight.
 const old=['old-crosshair','old-crosshair-carry'].map(role=>{const n=document.createElement('div');n.className='old-crosshair';n.hidden=true;const url=ui.urls[ui.layouts[role].sprite];n.style.maskImage=n.style.webkitMaskImage=`url("${url}")`;icon.append(n);return n;});
 function showWeapon(){weapon.hidden=!(weaponIcon&&state.crosshair&CROSSHAIR.WEAPON);}
 let lastLayers=-1,lastOld=-1,lastWeight=-1,lastFluid='',lastNotice='',lastCharge=-1,lastCarry='',lastDoor='',lastUse='',lastTune='',lastScroll='',lastPrimary='',lastCrank='',lastHelp='';
 // Bound key names by action, and the driving assist bits (1 auto clutch, 2 auto shift).
 let names={},assists=0;
 // Hint templates: `{action}` is the bound key name of a source action in brackets, `<word>` an `ls.w` word.
 // They fill once per binding or language change.
 const templates={translationHelp:'{rmb}+[<mouse>]: <move>\n{scrollup}: Distance\n{looklean}+{scrollup}: Posture',
  rotationHelp:'{rmb}+[<mouse>]: <move>\n{mmb}+[<mouse>]: <rotate>\n{rmb}+{mmb}+[<mouse>]: Roll\n{scrollup}: Distance\n{looklean}+{scrollup}: Posture',
  doorMove:'{lmb}+[<mouse>]: <move>',doorOpen:'{lmb}+[<mouse>]: <openclose>',useLmbSwitch:'{use} / {lmb}: <turnSwitch>',useSwitch:'{use}: <turnSwitch>',useOpen:'{use}: <openclose>',
  tuneLmb:'{use} / {lmb}+[<mouse>]: <tune>',tune:'{use}+[<mouse>]: <tune>',tuneOpen:'{use}+[<mouse>] / {scrollup}: <openclose>',scrollTune:'{scrollup}: <tune>',scrollOpen:'{scrollup}: <openclose>',
  crankLmb:'{use} / {lmb}+[<mouse>]: <crank>',crank:'{use}+[<mouse>]: <crank>'};
 const texts={};
 function fillTexts(){
  names=boundNames(settings.values.inputs,actions);
  const fill=t=>t.replace(/<(\w+)>/g,(_,w)=>language.word(w)).replace(/\{(\w+)\}/g,(_,a)=>'['+names[a]+']');
  for(const row of rows)row.n.textContent=fill(row.template);
  for(const [name,t] of Object.entries(templates))texts[name]=fill(t);
  lastCarry=lastDoor=lastUse=lastTune=lastScroll=lastPrimary=lastCrank=lastHelp='';
 }
 const counts=new Uint8Array(4);
 const destinations={use:'use',door:'use',pickup:'pickup',drop:'pickup',mount:'mount',dismount:'mount',fill:'fill',eat:'use',smoke:'use',takeOne:'use',putOn:'pickup',drink:'use',tune:'use',enter:'use',exit:'use',switch:'use',primary:'use',push:'pickup',grip:'pickup',release:'pickup',throw:'pickup',scroll:'use',heldSwitch:'use',wash:'use',scrub:'use',equip:'pickup',recenter:'pickup',releaseEquipment:'pickup',releaseEquipmentSeated:'pickup',crank:'use',hoseAttach:'mount',coilUp:'use',drain:'use',reload:'use',unload:'use',binocularsUse:'use',binocularsFocus:'use',computerTurnOn:'use',computerTurnOff:'use',computerUse:'use',computerConnect:'use',computerDisconnect:'use'};
 const families={use:0,pickup:1,mount:2,fill:3};
 function placeRows(){
  counts.fill(0);
  for(const row of rows){if(row.n.hidden)continue;const offset=counts[row.family]++;if(offset!==row.offset){row.n.style.top=(row.top+offset*row.line)+'px';row.offset=offset;}}
 }
 function resize(){
  for(const row of rows){const role=destinations[row.role],box=applyLayout(row.n,ui.layouts[role]);row.family=families[role];row.top=box.y;row.line=box.fontSize*1.4;row.offset=-1;}
  placeRows();
  const scale=applyLayout(icon,ui.layouts.reticle).scale;icon.style.left='50%';icon.style.top='50%';icon.style.width=icon.style.height='0';
  for(const [layer,[,width,height]] of layers.map((n,i)=>[n,crosshairLayers(0)[i]])){layer.style.width=width*scale+'px';layer.style.height=height*scale+'px';}
  for(const child of pickupChildren){child.style.width=layers[2].style.width;child.style.height=layers[2].style.height;}
  old.forEach((n,i)=>{const [,,width,height]=ui.layouts[i?'old-crosshair-carry':'old-crosshair'].rect;n.style.width=width*scale+'px';n.style.height=height*scale+'px';});
  const fluidBox=applyLayout(fluid,ui.layouts.fluid);fluid.style.top=(fluidBox.y+fluidBox.height/2)+'px';fluid.style.height='auto';fluid.style.width='max-content';fluid.style.left='50%';
  const helpBox=applyLayout(helpNode,ui.layouts.help);helpNode.style.height='auto';helpNode.style.top=helpBox.y+'px';
  const carryBox=applyLayout(carry,ui.layouts.carry);carry.style.left='auto';carry.style.right='8px';carry.style.width='max-content';carry.style.height='auto';carry.style.top='auto';carry.style.bottom=(innerHeight-carryBox.y-carryBox.height)+'px';
  notice.style.fontSize=ui.layouts.use.font_size*scale+'px';
  charge.style.fontSize=ui.layouts.use.font_size*scale+'px';charge.style.left='50%';charge.style.top=(innerHeight/2+120*scale)+'px';
 }
 function paint(){
  root.hidden=!state.visible;
  const v=settings.values,help=v.BHelp;
  // Source `BHelp` hides the prompts and the item help.
  for(const r of rows)r.n.hidden=!help||(r.computer?!(state.computer&r.computer):r.prompt?!(state.prompts&r.prompt):!(state.actions&r.bit)||(r.bit===ACTION.DROP&&Boolean(state.actions&ACTION.THROW))||(r.bit===ACTION.SWITCH&&Boolean(state.actions&ACTION.PRIMARY_SWITCH))||(r.bit===ACTION.MOUNT&&Boolean(state.prompts&PROMPT.ATTACH)));
  const doorText=state.context===7?texts.doorMove:texts.doorOpen;
  if(doorText!==lastDoor){named.door.textContent=doorText;lastDoor=doorText;}
  const useText=state.context===10?(state.lmbUse&&!(state.actions&ACTION.HELD_SWITCH)?texts.useLmbSwitch:texts.useSwitch):texts.useOpen;
  if(useText!==lastUse){named.use.textContent=useText;lastUse=useText;}
  const radio=state.context===11||state.context===12;
  const tuneText=radio?(state.lmbUse?texts.tuneLmb:texts.tune):texts.tuneOpen;
  if(tuneText!==lastTune){named.tune.textContent=tuneText;lastTune=tuneText;}
  const scrollText=radio?texts.scrollTune:texts.scrollOpen;
  if(scrollText!==lastScroll){named.scroll.textContent=scrollText;lastScroll=scrollText;}
  const primaryText=state.actions&ACTION.HELD_SWITCH?texts.useSwitch:texts.useLmbSwitch;
  if(primaryText!==lastPrimary){named.primary.textContent=primaryText;lastPrimary=primaryText;}
  const crankText=state.lmbUse?texts.crankLmb:texts.crank;
  if(crankText!==lastCrank){named.crank.textContent=crankText;lastCrank=crankText;}
  placeRows();
  charge.hidden=!state.throwActive;const strength=Math.max(0,Math.min(1,state.throwCharge||0));
  if(strength!==lastCharge){level.value=strength;lastCharge=strength;}
  carry.hidden=!help||!state.carrying;const carryText=state.actions&ACTION.ROTATE?texts.rotationHelp:texts.translationHelp;
  if(carryText!==lastCarry){carry.textContent=carryText;lastCarry=carryText;}
  // Source GhelpText: `BHelp` or `showifmenudisabledtoo`, and a help box under the use ray.
  const shown=Boolean(state.help)&&(help||Boolean(state.help&HELP.ALWAYS));helpNode.hidden=!shown;
  const helpLines=shown?helpText(state.help,assists,language.word,names,v.inputs[actions.lookright][0]==='Mouse X'):'';
  if(helpLines!==lastHelp){helpNode.textContent=helpLines;lastHelp=helpLines;}
  // Source `BCrosshair` hides every crosshair. `BOldCrosshair` swaps the layers for GC and GC2. TCrossHair scales both.
  icon.hidden=!v.BCrosshair;icon.style.scale=String(v.FCrosshairScale);
  for(const layer of layers)layer.style.visibility=v.BOldCrosshair?'hidden':'';
  for(const child of pickupChildren)child.style.visibility=v.BOldCrosshair?'hidden':'';
  const oldBits=v.BOldCrosshair?state.crosshair:-1;
  if(oldBits!==lastOld){
   lastOld=oldBits;old[0].hidden=oldBits<=0;old[1].hidden=oldBits<=0||!(oldBits&CROSSHAIR.PICKUP);
   if(oldBits>0)old[0].style.backgroundColor=css(oldCrosshairColor(oldBits,ui.crosshair_colors));
  }
  if(!old[1].hidden){const w=state.weight;old[1].style.backgroundColor=css([(.5-(w-.5))*2,w*2,0,.5]);}
  if(state.crosshair!==lastLayers){lastLayers=state.crosshair;crosshairLayers(state.crosshair).forEach(([role],i)=>{const layer=layers[i];if(layer===weapon)return;layer.hidden=role===null;if(role!==null&&layer.dataset.role!==String(role)){layer.src=ui.urls[typeof role==='number'?role:ui.icons[role]];layer.dataset.role=role;}});for(const child of pickupChildren)child.hidden=layers[2].hidden;showWeapon();}
  if(state.weight!==lastWeight){lastWeight=state.weight;pickupWeightAlphas(state.weight).forEach((alpha,i)=>{pickupChildren[i].style.opacity=alpha;});}
  if(state.fluidText!==lastFluid){fluid.textContent=state.fluidText;lastFluid=state.fluidText;}fluid.hidden=!state.fluidText;
  const message=state.context===6?'Unlimited Gas':state.notice;
  if(message!==lastNotice){notice.textContent=message;lastNotice=message;}notice.hidden=!message;
 }
 function setWeapon(sprite,color){
  if(sprite!==weaponIcon){weaponIcon=sprite;const url=sprite?ui.urls[sprite-1]:'';weapon.style.maskImage=weapon.style.webkitMaskImage=url?`url("${url}")`:'none';showWeapon();}
  weapon.style.backgroundColor=`rgba(${color&255},${color>>>8&255},${color>>>16&255},${(color>>>24&255)/255})`;
 }
 const inventory=document.createElement('div');inventory.id='inventory';inventory.setAttribute('aria-label','Inventory');root.append(inventory);
 const slotNodes=Array.from({length:4},(_,i)=>{const tile=document.createElement('div'),number=document.createElement('span'),icon=document.createElement('img');number.textContent=String(i+1);icon.hidden=true;tile.append(number,icon);inventory.append(tile);return {tile,icon,id:0};});
 const itemIcons=new Map(manifest.items.filter(i=>i.pickup?.can_inventory).map(i=>[i.id,{name:i.name,url:ui.urls[ui.sprites.findIndex(s=>s.source.file===i.pickup.inventory_icon?.file&&s.source.path_id===i.pickup.inventory_icon?.path_id)]}]));
 const swap=document.createElement('progress');swap.max=1;swap.setAttribute('aria-label','Swap progress');inventory.append(swap);
 function updateInventory(r){
  for(let i=0;i<4;i++){const n=slotNodes[i],id=r.slots[i];n.tile.classList.toggle('selected',r.selected===i);if(n.id!==id){n.id=id;const item=itemIcons.get(id);n.icon.hidden=!item;if(item){n.icon.src=item.url;n.icon.alt=item.name;}}}
  swap.hidden=!(r.progress>0);swap.value=Math.min(1,r.progress);
 }
 // Source `IN.hidehud` keys.
 const key=event=>{if(settings.values.inputs[actions.hidehud].includes(event.code)&&!event.repeat&&state.active){event.preventDefault();state.toggle();paint();}};
 let inputs=settings.values.inputs;fillTexts();
 settings.listen(v=>{if(v.inputs!==inputs){inputs=v.inputs;fillTexts();}paint();});
 language.listen(()=>{fillTexts();state.relabel();paint();});
 window.addEventListener('keydown',key);window.addEventListener('resize',resize);resize();paint();
 return {state,inventory:updateInventory,weapon:setWeapon,drivingState(value,seated,assistBits){if(value!==lastGear){gear.textContent='Gear: '+formatGear(value);lastGear=value;}if(seated!==wasSeated){gear.hidden=!seated;wasSeated=seated;}if(assistBits!==assists){assists=assistBits;paint();}},update(record){if(state.update(record))paint();},setVisible(value){const before=state.visible;state.setVisible(value);if(before!==state.visible)paint();},dispose(){window.removeEventListener('keydown',key);window.removeEventListener('resize',resize);}};
}
