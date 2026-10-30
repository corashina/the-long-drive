import {trimName,defaultName,AUTOSAVE} from './save-storage.js';
import {bindToggle} from './settings-screen.js';
export const PER_PAGE=6,STEP=2;
// Source `ls.w` lines of the cant-save texts.
const CANT_BOTH=[['cantSave','whileSitting','OrSleeping'],['cantSave','whileSleeping','OrSittingHere'],['dontsleep','andsit'],['dontsit','andsleep'],['keeptrying']];
export class SaveScreenState {
 // text starts at 4, so the first pick of both texts is from 0-3, as in the source.
 constructor(){this.saves=[];this.autosave=null;this.last=null;this.canLast=false;this.top=0;this.selected=null;this.confirm=null;this.name='';this.inMenu=true;this.dead=false;this.sleeping=false;this.seated=false;this.text=4;}
 set({saves,autosave,last},canLast=false){
  Object.assign(this,{saves,autosave,last,canLast});
  if(this.selected==='autosave'?!autosave:this.selected!==null&&this.selected>=saves.length)this.selected=null;
  this.clamp();
 }
 clamp(){this.top=Math.max(0,Math.min(this.top,this.saves.length-PER_PAGE));}
 scroll(up){this.top+=up?-STEP:STEP;this.clamp();}
 scrollFast(up){this.top+=up?-PER_PAGE:PER_PAGE;this.clamp();}
 jump(up){this.top=up?0:this.saves.length;this.clamp();}
 page(){const count=this.saves.length,pages=Math.max(1,Math.ceil(count/PER_PAGE));return {page:this.top>=count-PER_PAGE?pages:Math.floor(this.top/PER_PAGE)+1,pages,count};}
 entry(){return (this.selected==='autosave'?this.autosave:this.saves[this.selected])??null;}
 select(i){const entry=i==='autosave'?this.autosave:this.saves[i];if(!entry)return;this.selected=i;this.confirm=null;if(i!=='autosave')this.name=entry.name;}
 escape(){if(this.selected===null)return 'back';this.selected=null;this.confirm=null;return null;}
 saveShown(){return !this.inMenu&&!this.dead&&this.selected!=='autosave'&&!this.sleeping&&!this.seated;}
 canSave(){const name=trimName(this.name);return this.saveShown()&&name!==''&&name!==AUTOSAVE;}
 // Returns true when the operation runs now; otherwise it opens its Yes/No or does nothing.
 request(kind){
  this.confirm=null;
  if(kind==='save'){if(!this.canSave())return false;const name=trimName(this.name);if(this.saves.some(entry=>entry.name===name)){this.confirm='save';return false;}return true;}
  if(kind==='last'?!this.canLast:!this.entry())return false;
  if(this.inMenu&&kind!=='delete')return true;
  this.confirm=kind;return false;
 }
 answer(yes){const kind=this.confirm;this.confirm=null;return yes?kind:null;}
 cantSaveText(random=Math.random){
  if(this.sleeping&&this.seated){let i=Math.floor(random()*4);if(i>=this.text)i++;this.text=i;return CANT_BOTH[i];}
  if(this.sleeping)return ['cantSave','whileSleeping'];
  return this.seated?['cantSave','whileSitting']:[];
 }
}
// `settings` is the settings store. The autosave toggles are its source `tabMisc` toggles. `word` returns a source `ls.w` word.
export function createSaveScreen({settings,word,run,paint}){
 const driven=km=>String(Math.round(km)).padStart(4,'0')+' '+word('mkm');
 const details=entry=>[entry.date,word('mseed')+' '+entry.seed,driven(entry.drivenKm),entry.version].join(' · ');
 const state=new SaveScreenState(),q=selector=>document.querySelector(selector),urls=new Map(),kinds=['save','load','delete','last'];
 const tile=selector=>({button:q(selector),img:q(selector+' img'),label:q(selector+' strong'),details:q(selector+' small'),url:null});
 const tiles=Array.from({length:PER_PAGE},(_,i)=>tile('#save-tile-'+i)),auto=tile('#save-autosave');
 const name=q('#save-name'),info=q('#save-info'),cant=q('#save-cant'),pageText=q('#save-page'),countText=q('#save-count'),nameRow=q('#save-name-row');
 const fields=['name','date','seed','driven','version'].map(key=>q('#save-info-'+key));
 const toggles=[[q('#autosave-on'),'BAutoSave'],[q('#autosave-no-car'),'BDisableAutoSaveInCar'],[q('#auto-load'),'BAutoLoad']].map(([input,name])=>bindToggle(input,settings,name));
 let text='';
 function operate(kind){
  const entry=state.entry();
  if(kind==='save')run('save',trimName(state.name));
  else if(kind==='load')run('load',entry.name);
  else if(kind==='delete'){state.selected=null;run('remove',entry.name);}
  else run('loadLast');
 }
 function show(t,entry,selected){
  t.button.hidden=!entry;if(!entry)return;
  const url=urls.get(entry.name)??null;
  if(t.url!==url){t.url=url;if(url)t.img.src=url;else t.img.removeAttribute?.('src');}
  t.img.hidden=!url;t.label.textContent=entry.name;t.details.textContent=details(entry);
  t.button.classList.toggle('no-image',!url);t.button.classList.toggle('last',entry.name===state.last);t.button.classList.toggle('selected',selected);
 }
 tiles.forEach((t,i)=>t.button.addEventListener('click',()=>{state.select(state.top+i);paint();}));
 auto.button.addEventListener('click',()=>{state.select('autosave');paint();});
 for(const [id,move] of [['#save-up',()=>state.scroll(true)],['#save-down',()=>state.scroll(false)],['#save-page-up',()=>state.scrollFast(true)],['#save-page-down',()=>state.scrollFast(false)],['#save-top',()=>state.jump(true)],['#save-end',()=>state.jump(false)]])q(id).addEventListener('click',()=>{move();paint();});
 q('#save-tiles').addEventListener('wheel',event=>{event.preventDefault();if(event.deltaY)state.scroll(event.deltaY<0);paint();},{passive:false});
 name.addEventListener('input',()=>{state.name=name.value;state.confirm=null;paint();});
 for(const kind of kinds){
  q('#save-'+kind).addEventListener('click',()=>{if(state.request(kind))operate(kind);paint();});
  q('#save-'+kind+'-yes').addEventListener('click',()=>{const k=state.answer(true);if(k)operate(k);paint();});
  q('#save-'+kind+'-no').addEventListener('click',()=>{state.answer(false);paint();});
 }
 return {
  state,
  open(flags){
   Object.assign(state,flags,{top:0,selected:null,confirm:null,name:defaultName(new Date())});name.value=state.name;
   text=state.inMenu||state.dead?'':state.cantSaveText().map(word).join('\n');
  },
  update(list,canLast){
   for(const url of urls.values())URL.revokeObjectURL(url);urls.clear();
   for(const entry of [...list.saves,list.autosave])if(entry?.thumbnail)urls.set(entry.name,URL.createObjectURL(entry.thumbnail));
   state.set(list,canLast);
  },
  paint(pending){
   tiles.forEach((t,i)=>show(t,state.saves[state.top+i],state.selected===state.top+i));
   show(auto,state.autosave,state.selected==='autosave');
   const {page,pages,count}=state.page();pageText.textContent=word('mpage')+': '+page+'/'+pages;countText.textContent=word('mcount')+': '+count;
   const entry=state.entry();info.hidden=!entry;
   if(entry)[entry.name,entry.date,String(entry.seed),driven(entry.drivenKm),entry.version].forEach((value,i)=>{fields[i].textContent=value;});
   nameRow.hidden=!state.saveShown();name.disabled=pending;
   const shown={save:state.saveShown(),load:Boolean(entry),delete:Boolean(entry),last:state.canLast};
   for(const kind of kinds){
    const open=state.confirm===kind,button=q('#save-'+kind);
    button.hidden=!shown[kind]||open;button.disabled=pending||kind==='save'&&!state.canSave();
    q('#save-'+kind+'-confirm').hidden=!shown[kind]||!open;q('#save-'+kind+'-yes').disabled=pending;q('#save-'+kind+'-no').disabled=pending;
   }
   cant.hidden=!text;cant.textContent=text;
   for(const toggle of toggles)toggle();
  }
 };
}
