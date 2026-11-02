// Settings screen: source `settingsscript` tabs. Store rows are built from the exported settings record.
import {sliderSetting} from './settings.js';
// Rows of each tab in source order: a slider or toggle name, then an optional toggle on the same row.
const ROWS={
 graphics:[['BEnableFpsLimit'],['FFPSLimit']],
 controls:[['FMouseSensXY'],['FMouseSensX','BInvertMouseSensX'],['FMouseSensY','BInvertMouseSensY'],['FMouseSensZ','BInvertMouseSensZ'],
  ['FMouseMove','BInvertMouseMove'],['FMouseRot','BInvertMouseRot'],['FKeyLookSens'],['FKeySteerSens']],
 misc:[['FSound'],['FFieldOfViewFoot'],['FFieldOfViewCar'],['FCrosshairScale'],['BWalkHeadBob'],['BDriveHeadBob'],['BHelp'],['BCrosshair'],['BOldCrosshair'],
  ['BAlwaysShowHP'],['BAlwaysShowFood'],['BAlwaysShowWater'],['BAlwaysShowPiss'],['BAlwaysShowShit']],
};
// Source `ls.w` word of each row.
const LABELS={BEnableFpsLimit:'fpslimit',FFPSLimit:'fpslimit',FMouseSensXY:'mousesensitivity',FMouseSensX:'horizontalx',FMouseSensY:'verticaly',
 FMouseSensZ:'scrollwheelsensitivity',FMouseMove:'objectmovesensitivity',FMouseRot:'objectrotatesensitivity',FKeyLookSens:'keyboardlooksensitivity',FKeySteerSens:'keysteersens',
 BInvertMouseSensX:'swinvert',BInvertMouseSensY:'swinvert',BInvertMouseSensZ:'swinvert',BInvertMouseMove:'swinvert',BInvertMouseRot:'swinvert',
 FSound:'msound',FFieldOfViewFoot:'fieldofviewonfoot',FFieldOfViewCar:'fieldofviewincar',FCrosshairScale:'crosshairscale',
 BWalkHeadBob:'walkheadbob',BDriveHeadBob:'driveheadbob',BHelp:'controlshelp',BCrosshair:'crosshair',BOldCrosshair:'oldcrosshair',
 BAlwaysShowHP:'alwaysshowhp',BAlwaysShowFood:'alwaysshowfood',BAlwaysShowWater:'alwaysshowwater',BAlwaysShowPiss:'alwaysshowpiss',BAlwaysShowShit:'alwaysshowshit'};
export const TABS=['graphics','controls','steering','misc'];
// A toggle input that follows the store. A child locks while a parent is off.
export function bindToggle(input,settings,name){
 const parents=Object.entries(settings.reference.toggles).filter(([,t])=>t.children.includes(name)).map(([parent])=>parent);
 input.addEventListener('change',()=>settings.set(name,input.checked));
 return ()=>{input.checked=settings.values[name];input.disabled=parents.some(p=>!settings.values[p]);};
}
// `root` is the settings screen. `inMenu` is true before a game starts. `language` is the word table (`createLanguage`).
// `resets` adds the reset of the controls each tab holds outside the store.
export function createSettingsScreen(root,settings,language,inMenu,resets={}){
 const doc=root.ownerDocument,paints=[];
 const el=(tag,props,...children)=>{const node=Object.assign(doc.createElement(tag),props);node.append(...children);return node;};
 function toggle(name){
  const input=el('input',{type:'checkbox',id:'setting-'+name});
  paints.push(bindToggle(input,settings,name));
  return el('label',{className:'setting-toggle'},input,language.text(el('span'),LABELS[name]));
 }
 function slider(name){
  const s=settings.reference.sliders[name],id='setting-'+name;
  const range=el('input',{type:'range',id,min:String(s.slider[0]),max:String(s.slider[1]),step:s.whole?'1':'any'});
  const label=language.text(el('label',{htmlFor:id,id:id+'-label'}),LABELS[name]);
  const number=el('input',{type:'number',min:String(s.minimum),max:String(s.maximum),step:'any'});
  const reset=language.text(el('button',{type:'button'}),'mreset');
  number.setAttribute('aria-labelledby',label.id);reset.setAttribute('aria-describedby',label.id);
  range.addEventListener('input',()=>settings.set(name,sliderSetting(s,Number(range.value),true)));
  number.addEventListener('change',()=>{const value=Number(number.value);if(number.value!==''&&Number.isFinite(value))settings.set(name,sliderSetting(s,value,false));paint();});
  reset.addEventListener('click',()=>settings.set(name,s.default*s.multiplier));
  paints.push(()=>{
   const value=settings.values[name]/s.multiplier,locked=s.menu_only&&!inMenu();
   range.value=String(value);if(doc.activeElement!==number)number.value=String(value);
   range.disabled=number.disabled=reset.disabled=locked;
  });
  return el('div',{className:'setting-slider'},label,range,number,reset);
 }
 for(const [tab,rows] of Object.entries(ROWS)){
  const list=root.querySelector('[data-settings="'+tab+'"]');
  for(const names of rows)list.append(el('div',{className:'setting-row'},...names.map(name=>name in settings.reference.sliders?slider(name):toggle(name))));
 }
 const buttons=[...root.querySelectorAll('#settings-tabs button')],panels=[...root.querySelectorAll('.settings-tab')],resetButton=root.querySelector('#settings-reset');
 let tab='graphics';
 function paint(){
  for(const p of paints)p();
  for(const b of buttons){const on=b.dataset.tab===tab;b.classList.toggle('selected',on);b.setAttribute('aria-selected',String(on));}
  for(const p of panels)p.hidden=p.dataset.tab!==tab;
  // The steering wheel reset of the source is not ported.
  resetButton.hidden=tab==='steering';
 }
 for(const b of buttons)b.addEventListener('click',()=>{tab=b.dataset.tab;paint();});
 resetButton.addEventListener('click',()=>{settings.reset(tab,inMenu());for(const reset of resets[tab]??[])reset();});
 settings.listen(paint);
 paint();
 return {paint,tab:()=>tab};
}
