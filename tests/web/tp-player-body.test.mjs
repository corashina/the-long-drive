// Roadmap test pass rows of the Player body and outfit section.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createAppearance} from '../../web/appearance.js';
import {installOutfit} from '../../web/outfit.js';

const memory=items=>()=>({getItem:key=>items.get(key)??null,setItem:(key,value)=>items.set(key,value)});
// A small DOM: elements keep listeners, children, values and classes.
function element(tag){
 const listeners={},classes=new Set();
 return {tag,listeners,children:[],style:{},dataset:{},value:'',checked:false,
  addEventListener(type,f){(listeners[type]??=[]).push(f);},fire(type){for(const f of listeners[type]??[])f();},
  append(...nodes){this.children.push(...nodes);},replaceChildren(...nodes){this.children=nodes;},setAttribute(){},
  querySelectorAll(selector){const tag=selector,found=[];const walk=n=>{for(const c of n.children){if(c.tag===tag)found.push(c);walk(c);}};walk(this);return found;},
  classList:{toggle(name,on){if(on)classes.add(name);else classes.delete(name);}},
  getBoundingClientRect:()=>({left:0,top:0,width:100,height:100})};
}
function page(){
 const nodes={};
 const document={querySelector:s=>nodes[s]??=element(s),createElement:element};
 return {document,nodes};
}
const characters=[0,1].map(c=>({segments:[0,1].map(i=>({default_enabled:true,default_hsv:[0.1*i,0.5,0.8],language_name:i,disable_able:i===1}))}));
const language={index:i=>`part ${i}`,listen(){}};

// Source `settingsplayercustomscript`: `SlidersChanged` marks the character edited, `PressedToggle` turns a
// part off, `OnDisable` runs `SaveSettings`. `LoadSettings` keeps edited characters and resets the others,
// so a toggle without a color change is lost on reload, in the source too.
test('tp_pb16_pb17_outfit_colors_and_parts_save_on_close_and_come_back_after_a_reload',()=>{
 const items=new Map(),target={};
 const edit=installOutfit(target,memory(items),characters);
 const {document,nodes}=page();
 const appearance=createAppearance({target,document,canvas:element('canvas'),edit,characters,language});
 assert.equal(JSON.parse(target.gameOutfit()).characters[0].edited,false);
 appearance.open();
 // Female (character 1): part 1 off, part 0 hue 40.
 nodes['#appearance-character'].value='1';nodes['#appearance-character'].fire('change');
 const rows=nodes['#appearance-parts'].children;
 const toggle=rows[1].children[1];toggle.checked=false;toggle.fire('change');
 nodes['#appearance-hue'].value='40';nodes['#appearance-hue'].fire('input');
 // Each change reaches Rust at once.
 const sent=JSON.parse(target.gameOutfit());
 assert.equal(sent.character,1);
 assert.deepEqual(sent.characters[1].segments.map(s=>[s.enabled,s.hue]),[[true,0.4],[false,0.1]]);
 assert.equal(items.size,0);
 // Back or Escape closes the screen: menu.js `paint` calls `close`.
 appearance.close();
 const reloaded={};installOutfit(reloaded,memory(items),characters);
 const back=JSON.parse(reloaded.gameOutfit());
 assert.deepEqual(back,sent);
 // The untouched character keeps its defaults.
 assert.deepEqual(back.characters[0].segments.map(s=>[s.enabled,s.hue]),[[true,0],[true,0.1]]);
});
