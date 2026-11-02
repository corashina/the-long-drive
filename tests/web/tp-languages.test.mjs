// Roadmap test pass rows of the Languages and help text section.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createSettings} from '../../web/settings.js';
import {createLanguage,languageField,languageNames,wordTable} from '../../web/language.js';
import {HINTS,formatRadio,formatTank} from '../../web/feedback.js';
import {HELP,SETS,helpText} from '../../web/help-text.js';
import {actionIndex,boundNames,inputsField} from '../../web/bindings.js';
import {NO_MANIFEST,reference} from './reference.mjs';

const memory=items=>()=>({getItem:key=>items.get(key)??null,setItem:(key,value)=>items.set(key,value)});
const settingsOf=(m,items)=>createSettings(memory(items),m.settings,{inputs:inputsField(m.driving.bindings),language:languageField(m.language)});
const FILE_LANGUAGES=['中国人','čeština','Suomalainen','Deutsch','Magyar','italiano','日本語','русский','Türk'];

// Source `ls.ScanLanguages` (English, then the files) and `ls.UpdLanguage` (every text at once).
test('tp_l2_the_list_holds_english_then_9_files_and_deutsch_changes_at_once_and_stays',{skip:NO_MANIFEST},()=>{
 const m=reference(),items=new Map();
 assert.deepEqual(languageNames(m.language),['English',...FILE_LANGUAGES]);
 const nodes=['mback','msettings','mload'].map(w=>({dataset:{w},textContent:''}));
 const doc={querySelectorAll:()=>nodes};
 const settings=settingsOf(m,items),language=createLanguage(m.language,settings,doc);
 let changes=0;language.listen(()=>changes++);
 assert.deepEqual(nodes.map(n=>n.textContent),nodes.map(n=>wordTable(m.language,'English')[n.dataset.w]));
 settings.set('language','Deutsch');
 const de=wordTable(m.language,'Deutsch');
 assert.deepEqual(nodes.map(n=>n.textContent),nodes.map(n=>de[n.dataset.w]));
 assert.notDeepEqual(nodes.map(n=>n.textContent),nodes.map(n=>wordTable(m.language,'English')[n.dataset.w]));
 assert.equal(changes,1);
 // Reload: a new store on the same storage keeps Deutsch.
 assert.equal(settingsOf(m,items).values.language,'Deutsch');
});

// The code points of a TrueType font (cmap formats 4 and 12).
function codePoints(font){
 const v=new DataView(font.buffer,font.byteOffset,font.byteLength),points=new Set();
 let cmap=0;for(let i=0;i<v.getUint16(4);i++)if(v.getUint32(12+i*16)===0x636d6170)cmap=v.getUint32(12+i*16+8);
 for(let i=0;i<v.getUint16(cmap+2);i++){
  const t=cmap+v.getUint32(cmap+4+i*8+4),format=v.getUint16(t);
  if(format===4){
   const segs=v.getUint16(t+6)/2,ends=t+14,starts=ends+segs*2+2,deltas=starts+segs*2,ranges=deltas+segs*2;
   for(let s=0;s<segs;s++){
    const end=v.getUint16(ends+s*2),start=v.getUint16(starts+s*2),delta=v.getInt16(deltas+s*2),range=v.getUint16(ranges+s*2);
    for(let c=start;c<=end&&c!==0xffff;c++){
     const glyph=range?v.getUint16(ranges+s*2+range+(c-start)*2):(c+delta)&0xffff;
     if(glyph)points.add(c);
    }
   }
  }else if(format===12){
   for(let g=0;g<v.getUint32(t+12);g++){const at=t+16+g*12;for(let c=v.getUint32(at);c<=v.getUint32(at+4);c++)points.add(c);}
  }
 }
 return points;
}
// CJK, kana and full-width forms. The menu font has none; the browser takes them from its sans-serif font.
// U+23CE is in the `xsend` word only, which the port does not show.
const fallback=c=>c>=0x2e80&&c<=0x9fff||c>=0xf900&&c<=0xfaff||c>=0xff00&&c<=0xffef||c>=0x3000&&c<=0x30ff||c===0x23ce;
test('tp_l3_the_menu_font_covers_every_word_table_except_cjk',{skip:NO_MANIFEST},()=>{
 const points=codePoints(readFileSync(new URL('../../web/fonts/LiberationSans-Regular.ttf',import.meta.url)));
 assert.ok(points.has(0x41)&&points.has(0x416)&&!points.has(0x65e5));
 const missing={};
 for(const name of languageNames(reference().language)){
  const chars=new Set([...Object.values(wordTable(reference().language,name)).join('')].map(c=>c.codePointAt(0)).filter(c=>c>=0x20&&!points.has(c)));
  const other=[...chars].filter(c=>!fallback(c));
  if(other.length)missing[name]=other.map(c=>String.fromCodePoint(c)+' U+'+c.toString(16));
  if(chars.size)console.log(`${name}: ${chars.size} code points from the fallback font, ${other.length} not CJK`);
 }
 assert.deepEqual(missing,{});
});

// Source `GetCapString`, `radioscript` texts and `menu.TDistance` with the Deutsch word table.
test('tp_l4_tanks_radio_and_distance_use_the_german_words',{skip:NO_MANIFEST},()=>{
 const m=reference(),de=wordTable(m.language,'Deutsch'),word=w=>de[w];
 const index=i=>de[m.language.names[i]];
 const tank={context:0,noText:false,hasName:true,fluidWord:m.language.names.indexOf('gas'),amount:10,capacity:20,fluidOrder:0x21,quantities:[5,5,0,0,0,0]};
 assert.equal(formatTank(tank,word,index),'Benzin:\n10 / 20 L\n50% Benzin\n50% Öl');
 assert.equal(formatTank({...tank,amount:20,quantities:[20,0,0,0,0,0],fluidOrder:1},word,index),'Benzin:\n20 L (voll)\n100% Benzin');
 assert.equal(formatTank({...tank,amount:0},word,index),'Benzin:\nleer');
 assert.equal(formatRadio({context:11,radioVolume:.5},word),'Volumen: 05');
 assert.equal(formatRadio({context:12,radioFrequency:101.5,radioAm:false},word),de.fm+' 101.5');
 assert.equal(de.mdistancedriven,'Gesteuerte Entfernung');
 for(const w of ['volume','am','fm','liter','full','empty','percent','gas','oil','alcohol','water','blood','diesel','mdistancedriven','mkm'])assert.ok(de[w],w);
 // Each hint word has a German word. English literals (Distance, Posture, Roll) have no source `ls.w` word.
 for(const t of Object.values(HINTS))for(const [,w] of t.matchAll(/<(\w+)>/g))assert.ok(de[w]&&m.language.languages.find(l=>l.name==='Deutsch').words[m.language.names.indexOf(w)],w);
});

// Source `mainscript.RefreshHelpTextLanguage` and `RefreshHelpTexts`. Input indices are source `IN` values.
const SOURCE_IN={looklean:20,scrollup:35,gas:49,brake:50,steerleft:51,steerright:52,clutch:53,handbrake:54,shiftup:55,shiftdown:56,gearr:57,gearn:58,gear1:59,gear4:62,ignition:65,lights:67,lookright:7};
test('tp_l5_a_car_help_box_follows_auto_clutch_and_auto_shift_with_bound_keys',{skip:NO_MANIFEST},()=>{
 const m=reference(),en=wordTable(m.language,'English'),word=w=>en[w];
 const index=actionIndex(m.driving.bindings),settings=settingsOf(m,new Map()),names=boundNames(settings.values.inputs,index);
 for(const action of Object.keys(SOURCE_IN))assert.ok(action in index,action);
 const car=m.world.prototypes.find(p=>p.nodes[0].name==='Car02Full').help_texts[0];
 assert.deepEqual([car.force_auto,car.motorcycle,car.bike],[false,false,false]);
 const sets=[0,1,3].map(a=>helpText(HELP.SHOWN,a,word,names,true));
 assert.deepEqual(sets.map(s=>s.split('\n').length-1),[8,7,8]);
 assert.equal(new Set(sets).size,3);
 assert.equal(sets[0].split('\n')[0],`${en.ignition}: [${names.ignition}]  `);
 assert.equal(sets[1].split('\n')[2],`${en.upshift}: [${names.shiftup}]  `);
 assert.equal(sets[2].split('\n')[1],`${en.accelerate}: [${names.gas}]  `);
 // Auto shift without auto clutch keeps the manual set, as in the source.
 assert.equal(helpText(HELP.SHOWN,2,word,names,true),sets[0]);
});

// Source `helptextscript.ForceAuto`, `isMotorcycle` and `isBike` on the exported prototypes.
test('tp_l6_buses_and_motorcycles_take_their_auto_shift_sets',{skip:NO_MANIFEST},()=>{
 const m=reference(),en=wordTable(m.language,'English'),word=w=>en[w];
 const names=boundNames(settingsOf(m,new Map()).values.inputs,actionIndex(m.driving.bindings));
 const bits=name=>{const h=m.world.prototypes.find(p=>p.nodes[0].name===name).help_texts[0];return HELP.SHOWN|(h.force_auto?HELP.FORCE_AUTO:0)|(h.motorcycle?HELP.MOTORCYCLE:0)|(h.bike?HELP.BIKE:0);};
 const autoShift=helpText(HELP.SHOWN,3,word,names,true);
 for(const bus of ['Bus01Full','Bus03Full'])for(const a of [0,1,3])assert.equal(helpText(bits(bus),a,word,names,true),autoShift,bus);
 // The source Bus02 box has no ForceAuto: it follows the assists.
 assert.equal(bits('Bus02Full'),HELP.SHOWN);
 for(const bike of ['Bike01Full','Bike03Full'])for(const a of [0,1,3])assert.equal(helpText(bits(bike),a,word,names,true).split('\n').length-1,SETS.motorcycle[2].length,bike);
});
