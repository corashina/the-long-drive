// Player outfit (`settingsplayercustomscript.outfitclass`) in browser storage, sent to Rust as `Outfit` JSON.
// This file is the one owner of the outfit and its storage key.
// `characters` are the manifest `clothing.characters`. Each has `segments` with `default_enabled` and `default_hsv`.
const KEY='the-long-slop.outfit';
// `enabledCharacter`: 0 no body, 1 shadow only, 2 visible.
export const MODELS=3,VISIBLE=2;
// True when `value` is an object with exactly these fields, like Rust `deny_unknown_fields`.
const shaped=(value,fields)=>value!==null&&typeof value==='object'&&!Array.isArray(value)&&Object.keys(value).length===fields.length&&fields.every(f=>Object.hasOwn(value,f));
const unit=value=>Number.isFinite(value)&&value>=0&&value<=1;
const index=(value,limit)=>Number.isInteger(value)&&value>=0&&value<limit;
// `ResetOutfit`: the segment defaults. `default_hsv` is [hue, saturation, brightness].
export function resetCharacter(character){
 return {edited:false,segments:character.segments.map(s=>({enabled:s.default_enabled,hue:s.default_hsv[0],brightness:s.default_hsv[2],saturation:s.default_hsv[1]}))};
}
// `FullReset`: visible model, character 0, and each character reset.
export function fullReset(characters){return {model:VISIBLE,character:0,characters:characters.map(resetCharacter)};}
// The Rust `Outfit::valid` rules and the JSON shape.
export function validOutfit(value,characters){
 return shaped(value,['model','character','characters'])&&index(value.model,MODELS)&&index(value.character,characters.length)
  &&Array.isArray(value.characters)&&value.characters.length===characters.length
  &&value.characters.every((c,i)=>shaped(c,['edited','segments'])&&typeof c.edited==='boolean'
   &&Array.isArray(c.segments)&&c.segments.length===characters[i].segments.length
   &&c.segments.every(s=>shaped(s,['enabled','hue','brightness','saturation'])&&typeof s.enabled==='boolean'&&unit(s.hue)&&unit(s.brightness)&&unit(s.saturation)));
}
// `LoadSettings`: bad or missing data resets all. A character that is not edited resets to its defaults.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
export function readOutfit(storage,characters){
 let stored;try{stored=JSON.parse(storage().getItem(KEY));}catch{}
 if(!validOutfit(stored,characters))return fullReset(characters);
 stored.characters=stored.characters.map((c,i)=>c.edited?c:resetCharacter(characters[i]));
 return stored;
}
export function writeOutfit(storage,outfit){try{storage().setItem(KEY,JSON.stringify(outfit));}catch{}}
// Rust takes the outfit with `window.gameOutfit()` at start and after each change.
export function installOutfit(target,storage,characters){
 const outfit=readOutfit(storage,characters);
 let pending=true;
 target.gameOutfit=()=>{if(!pending)return undefined;pending=false;return JSON.stringify(outfit);};
 // Call `changed` after each change to `outfit`. `save` stores it (`SaveSettings`).
 return {outfit,changed(){pending=true;},save(){writeOutfit(storage,outfit);}};
}
