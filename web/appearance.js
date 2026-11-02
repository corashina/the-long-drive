// Appearance editor (`settingsplayercustomscript`): player model, character, part toggles and HSV sliders.
// It edits the `outfit.js` outfit. Each change goes to Rust at once. Close saves it (`OnDisable`).
import {resetCharacter} from './outfit.js';
// Unity `Color.HSVToRGB(h, s, v)` for values in 0..1.
export function hsvToRgb(h,s,v){
 const h6=h*6,i=Math.floor(h6)%6,f=h6-Math.floor(h6),p=v*(1-s),q=v*(1-s*f),t=v*(1-s*(1-f));
 return [[v,t,p],[q,v,p],[p,v,t],[p,q,v],[t,p,v],[v,p,q]][i];
}
const css=([r,g,b])=>'rgb('+[r,g,b].map(c=>Math.round(c*255)).join(' ')+')';
// `edit` is the `installOutfit` result. `characters` are the manifest `clothing.characters`.
export function createAppearance({target,document,canvas,edit,characters,language}){
 const {outfit}=edit,$=selector=>document.querySelector(selector);
 const model=$('#appearance-model'),character=$('#appearance-character'),parts=$('#appearance-parts'),view=$('#appearance-view');
 const sliders={hue:$('#appearance-hue'),brightness:$('#appearance-brightness'),saturation:$('#appearance-saturation')};
 let open=false,selected=0;
 // The preview rect in canvas fractions: x, y, width, height.
 const rect=[0,0,1,1];
 function measure(){
  if(!open)return;
  const c=canvas.getBoundingClientRect(),v=view.getBoundingClientRect();
  if(!c.width||!c.height)return;
  rect[0]=(v.left-c.left)/c.width;rect[1]=(v.top-c.top)/c.height;rect[2]=v.width/c.width;rect[3]=v.height/c.height;
 }
 // Rust reads the preview state once per frame.
 target.gameAppearancePreview=out=>{if(!open)return false;out[0]=rect[0];out[1]=rect[1];out[2]=rect[2];out[3]=rect[3];return true;};
 target.addEventListener?.('resize',measure);
 const current=()=>outfit.characters[outfit.character];
 // `GetColor`: the track colors of the selected part.
 function tracks(){
  const {hue,brightness,saturation}=current().segments[selected];
  sliders.hue.style.background='linear-gradient(to right,red,yellow,lime,cyan,blue,magenta,red)';
  sliders.brightness.style.background='linear-gradient(to right,#000,'+css(hsvToRgb(hue,saturation,1))+')';
  sliders.saturation.style.background='linear-gradient(to right,'+css([brightness,brightness,brightness])+','+css(hsvToRgb(hue,1,brightness))+')';
 }
 // `PressedColor`: select a part and show its values.
 function select(index){
  selected=index;
  const segment=current().segments[index];
  for(const key of Object.keys(sliders))sliders[key].value=String(Math.round(segment[key]*100));
  parts.querySelectorAll('button').forEach((button,i)=>button.classList.toggle('selected',i===index));
  tracks();
 }
 // `UpdButtons`: two columns; the first column takes the extra part.
 function list(){
  const segments=characters[outfit.character].segments;
  parts.style.gridTemplateRows='repeat('+Math.ceil(segments.length/2)+',auto)';
  parts.replaceChildren(...segments.map((segment,i)=>{
   const row=document.createElement('div'),button=document.createElement('button');
   button.type='button';button.textContent=language.index(segment.language_name);button.addEventListener('click',()=>select(i));
   row.append(button);
   if(segment.disable_able){
    const toggle=document.createElement('input');toggle.type='checkbox';toggle.checked=current().segments[i].enabled;
    toggle.setAttribute('aria-label',button.textContent);
    // `PressedToggle`.
    toggle.addEventListener('change',()=>{current().segments[i].enabled=toggle.checked;edit.changed();});
    row.append(toggle);
   }
   return row;
  }));
  select(Math.min(selected,segments.length-1));
 }
 function paint(){model.value=String(outfit.model);character.value=String(outfit.character);list();}
 // `DropDownUpd`.
 model.addEventListener('change',()=>{outfit.model=Number(model.value);edit.changed();});
 language.listen(list);
 character.addEventListener('change',()=>{outfit.character=Number(character.value);edit.changed();list();});
 // `SlidersChanged`: a change marks the character as edited (`noReset`).
 for(const [key,slider] of Object.entries(sliders))slider.addEventListener('input',()=>{
  const c=current();c.edited=true;c.segments[selected][key]=Number(slider.value)/100;tracks();edit.changed();
 });
 // `PressedReset`: `ResetOutfit` of the shown character.
 $('#appearance-reset').addEventListener('click',()=>{outfit.characters[outfit.character]=resetCharacter(characters[outfit.character]);edit.changed();list();});
 return {
  get isOpen(){return open;},
  // `OnEnable`.
  open(){open=true;paint();measure();},
  // `OnDisable`: `SaveSettings`.
  close(){if(!open)return;open=false;edit.save();},
 };
}
