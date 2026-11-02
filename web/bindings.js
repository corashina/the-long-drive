// Source `inputscript.IN` bindings: the settings store field `inputs` (source `S.inputs`), key names and the input maps.
// `defaults` is the exported `driving.bindings`: {action, keys} per action, in source enum order.
// A key is a KeyboardEvent.code, `Mouse0` to `Mouse2`, a mouse axis named like `oneKey.GetName` ('Mouse X', '-Mouse Y', 'Mouse ScrollWheel'), or null (None).
export const KEYS=3;
// Unity mouse button names by DOM `MouseEvent.button`: 0 left, 1 middle, 2 right.
export const MOUSE_BUTTONS=['Mouse0','Mouse2','Mouse1'];
// Port key bits of each action (crates/simulation/src/simulation.rs `keys`, `player_keys` and the inventory bits).
// Pickup, mount and push share the port grab bit. Actions without a bit only show in the list.
const KEY_BITS={forward:1,back:2,left:4,right:8,jump:16,use:32,run:64,duck:128,pickup:256,mountdismount:256,push:256,lmb:512,ignition:1024,camera:2048,mmb:4096,reload:8192,rmb:16384,equiprecenter:32768,lights:65536,
 horn:2**17,sleep:2**18,piss:2**19,burpfart:2**20,clutch:2**21,shiftup:2**22,shiftdown:2**23,gearr:2**24,gearn:2**25,gear1:2**26,gear2:2**27,gear3:2**28,gear4:2**29,gear5:2**30,gear6:2**31};
const PLAYER_BITS={looklean:1,zoom:2,smoke:4,removeSunglasses:8,togglemousedrive:16,pointrubwave:32,map:64,keylook:128,gas:256,brake:512,steerleft:1024,steerright:2048,handbrake:4096};
const INVENTORY_BITS={inventory1:1,inventory2:2,inventory3:4,inventory4:8};
// Source `GetAxis` actions, in `InputState` order: look X, look Y and scroll.
export const ANALOG=['lookright','lookleft','lookup','lookdown','scrollup','scrolldown'];
const copy=inputs=>inputs.map(keys=>keys.slice());
// The settings store field. Stored keys from another action list fall back to the defaults.
export function inputsField(defaults){
 const value=defaults.map(b=>b.keys);
 return {default:value,valid:v=>Array.isArray(v)&&v.length===value.length&&v.every(keys=>Array.isArray(keys)&&keys.length===KEYS&&keys.every(k=>k===null||typeof k==='string'&&k.length>0&&k.length<=32))};
}
// Source `settingsscript.PressedControl` then `ScanControls`, `PressedNone` and `ResetControls`.
export function setKey(inputs,action,key,code){const next=copy(inputs);next[action][key]=code;return next;}
export function clearAction(inputs,action){const next=copy(inputs);next[action].fill(null);return next;}
export function resetAction(inputs,defaults,action){const next=copy(inputs);next[action]=defaults[action].keys.slice();return next;}
export function resetInputs(defaults){return defaults.map(b=>b.keys.slice());}
const UNITY={ShiftLeft:'LeftShift',ShiftRight:'RightShift',ControlLeft:'LeftControl',ControlRight:'RightControl',AltLeft:'LeftAlt',AltRight:'RightAlt',MetaLeft:'LeftWindows',MetaRight:'RightWindows',
 ArrowUp:'UpArrow',ArrowDown:'DownArrow',ArrowLeft:'LeftArrow',ArrowRight:'RightArrow',NumpadAdd:'KeypadPlus',NumpadSubtract:'KeypadMinus',NumpadMultiply:'KeypadMultiply',NumpadDivide:'KeypadDivide',
 NumpadEnter:'KeypadEnter',NumpadDecimal:'KeypadPeriod',NumpadEqual:'KeypadEquals',Enter:'Return',Equal:'Equals',BracketLeft:'LeftBracket',BracketRight:'RightBracket',Backquote:'BackQuote',
 Mouse0:'LMB',Mouse1:'RMB',Mouse2:'MMB','Mouse ScrollWheel':'Scroll','-Mouse ScrollWheel':'-Scroll'};
// Source `oneKey.GetName`: Unity KeyCode names, the mouse button and scroll words, and None.
export function keyName(key){
 if(key===null)return 'None';
 if(UNITY[key])return UNITY[key];
 const m=/^(Key|Digit|Numpad)(.+)$/.exec(key);
 return m?(m[1]==='Key'?'':m[1]==='Digit'?'Alpha':'Keypad')+m[2]:key;
}
// Source `oneInput.GetName`: the first key that is not None.
export function actionName(keys){for(const k of keys)if(k!==null)return keyName(k);return 'None';}
// Source `oneInput.GetName` of each action by source name, for the HUD hints.
export function boundNames(inputs,index){const names={};for(const action in index)names[action]=actionName(inputs[index[action]]);return names;}
// Action index by source name.
export function actionIndex(defaults){return Object.fromEntries(defaults.map((b,i)=>[b.action,i]));}
// Bit maps by key for `InputState`, and the keys of each `ANALOG` action. Build once per change.
export function compileBindings(defaults,inputs){
 const maps={bits:{},playerBits:{},inventoryBits:{},analog:ANALOG.map(()=>[])},index=actionIndex(defaults);
 for(const [action,i] of Object.entries(index)){
  const a=ANALOG.indexOf(action);
  for(const key of inputs[i]){
   if(key===null)continue;
   if(a>=0){maps.analog[a].push(key);continue;}
   // A mouse axis acts only on the look and scroll actions.
   if(/^-?Mouse /.test(key))continue;
   if(action in KEY_BITS)maps.bits[key]=((maps.bits[key]??0)|KEY_BITS[action])>>>0;
   if(action in PLAYER_BITS)maps.playerBits[key]=(maps.playerBits[key]??0)|PLAYER_BITS[action];
   if(action in INVENTORY_BITS)maps.inventoryBits[key]=(maps.inventoryBits[key]??0)|INVENTORY_BITS[action];
  }
 }
 return maps;
}
