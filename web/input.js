import {MOUSE_BUTTONS} from './bindings.js';
// Typing keys (Rust TypingInput): piano bits 0-16 (computerscript order), arrows 17-20, music keys A S X 21-23, any key 24.
const TYPING_HELD={KeyQ:1,Digit2:2,KeyW:4,Digit3:8,KeyE:16,KeyR:32,Digit5:64,KeyT:128,Digit6:256,KeyY:512,KeyZ:512,Digit7:1024,KeyU:2048,KeyI:4096,Digit9:8192,KeyO:16384,Digit0:32768,KeyP:65536,ArrowUp:1<<17,ArrowDown:1<<18,ArrowLeft:1<<19,ArrowRight:1<<20};
const TYPING_PRESSED={KeyA:1<<21,KeyS:1<<22,KeyX:1<<23},TYPING_ANY=1<<24,TEXT_CAPACITY=64;
export class InputState {
 // `maps` is `compileBindings` output. A mouse axis or a key on a look or scroll action gives a `GetAxis` value per frame.
 constructor(maps){this.typing=false;this.typedCodes=new Set();this.text=new Uint8Array(TEXT_CAPACITY);this.textLength=0;this.typingPressed=0;this.codes=new Set();this.frame={held:0,pressed:0,released:0,mouseX:0,mouseY:0,wheel:0,driveX:0,driveY:0,inventoryHeld:0,inventoryReleased:0,playerHeld:0,playerPressed:0,playerReleased:0};this.setMaps(maps);}
 setMaps(maps){this.bits=maps.bits;this.playerBits=maps.playerBits;this.inventoryBits=maps.inventoryBits;this.analog=maps.analog;this.bound=new Set([...Object.keys(maps.bits),...Object.keys(maps.playerBits),...Object.keys(maps.inventoryBits),...maps.analog.flat()]);this.clear();}
 known(code){return this.bound.has(code);}
 code(code,down){if(!this.bound.has(code))return false;const old=this.held,oldInventory=this.inventoryHeld,oldPlayer=this.playerHeld;if(down)this.codes.add(code);else this.codes.delete(code);this.held=this.inventoryHeld=this.playerHeld=0;for(const k of this.codes){this.held=(this.held|(this.bits[k]??0))>>>0;this.inventoryHeld|=this.inventoryBits[k]??0;this.playerHeld|=this.playerBits[k]??0;}this.playerPressed|=this.playerHeld&~oldPlayer;this.playerReleased|=oldPlayer&~this.playerHeld;this.inventoryReleased|=oldInventory&~this.inventoryHeld;this.pressed=(this.pressed|(this.held&~old))>>>0;this.released=(this.released|(old&~this.held))>>>0;return true;}
 /** Source `oneInput.GetF` of `ANALOG[i]`: the largest key value. A mouse axis counts only in its direction. */
 axis(i){const keys=this.analog[i];let f=0;for(let k=0;k<keys.length;k++){const key=keys[k];let v;switch(key){case 'Mouse X':v=this.mouseX;break;case '-Mouse X':v=-this.mouseX;break;case 'Mouse Y':v=-this.mouseY;break;case '-Mouse Y':v=this.mouseY;break;case 'Mouse ScrollWheel':v=this.wheel;break;case '-Mouse ScrollWheel':v=-this.wheel;break;default:v=this.codes.has(key)?1:0;}if(v>f)f=v;}return f;}
 mouse(x,y){if(Number.isFinite(x))this.mouseX+=x;if(Number.isFinite(y))this.mouseY+=y;}
 scroll(value){if(Number.isFinite(value))this.wheel+=value;}
 /** Route the keyboard to a computer (Rust ComputerPrompt.TYPING). Game keys release. */
 setTyping(on){if(on===this.typing)return;this.typing=on;this.clear();}
 /** Input.inputString and the held typing keys. Repeats only add space and Backspace. */
 type(code,key,repeat){if(!repeat){this.typedCodes.add(code);this.typingPressed|=TYPING_PRESSED[code]??0;}const c=key==='Enter'?13:key==='Backspace'?8:key.length===1?Math.min(key.charCodeAt(0),127):0;if(c&&(!repeat||c===32||c===8)&&this.textLength<TEXT_CAPACITY)this.text[this.textLength++]=c;}
 untype(code){this.typedCodes.delete(code);}
 typingKeys(){let keys=this.typingPressed|(this.typedCodes.size?TYPING_ANY:0);for(const c of this.typedCodes)keys|=TYPING_HELD[c]??0;this.typingPressed=0;return keys;}
 takeText(buffer){const n=Math.min(this.textLength,buffer.length);buffer.set(this.text.subarray(0,n));this.textLength=0;return n;}
 clear(){this.typedCodes.clear();this.textLength=0;this.typingPressed=0;this.codes.clear();this.inventoryHeld=this.inventoryReleased=0;this.playerHeld=this.playerPressed=this.playerReleased=0;this.held=this.pressed=this.released=this.mouseX=this.mouseY=this.wheel=0;}
 // `mouseX`, `mouseY` are the look actions (browser +Y is down). `wheel` is the scroll actions. `driveX`, `driveY` are the raw mouse for mouse driving.
 takeFrame(){const f=this.frame;f.held=this.held;f.pressed=this.pressed;f.released=this.released;f.mouseX=this.axis(0)-this.axis(1);f.mouseY=this.axis(3)-this.axis(2);f.wheel=this.axis(4)-this.axis(5);f.driveX=this.mouseX;f.driveY=this.mouseY;f.inventoryHeld=this.inventoryHeld;f.inventoryReleased=this.inventoryReleased;f.playerHeld=this.playerHeld;f.playerPressed=this.playerPressed;f.playerReleased=this.playerReleased;this.playerPressed=this.playerReleased=0;this.inventoryReleased=0;this.pressed=this.released=this.mouseX=this.mouseY=this.wheel=0;return f;}
}
export function createInput(canvas,acceptInput,maps,profile) {
 const state=new InputState(maps);const listeners=[];
 let sequence=0,frameSequence=0;
 const mark=e=>{if(profile?.enabled)profile.input(++sequence,e.timeStamp,e.isTrusted);};
 const clear=()=>{state.clear();frameSequence=sequence;profile?.clearInput();};
 const listen=(node,event,handler,options)=>{node.addEventListener(event,handler,options);listeners.push([node,event,handler,options]);};
  listen(window,'keydown',e=>{if(acceptInput()&&state.typing&&e.code!=='Escape'){e.preventDefault();e.stopImmediatePropagation();state.type(e.code,e.key,e.repeat);mark(e);return;}if(acceptInput() && state.known(e.code)){e.preventDefault();if(!e.repeat){state.code(e.code,true);mark(e);}}});
  listen(window,'keyup',e=>{state.untype(e.code);if(state.known(e.code)){if(acceptInput())e.preventDefault();state.code(e.code,false);mark(e);}});
 listen(canvas,'mousedown',e=>{if(acceptInput() && e.button<=2){e.preventDefault();state.code(MOUSE_BUTTONS[e.button],true);mark(e);}});
 listen(window,'mouseup',e=>{if(e.button<=2){state.code(MOUSE_BUTTONS[e.button],false);mark(e);}});
  listen(canvas,'mousemove',e=>{if(acceptInput()){state.mouse(e.movementX*0.12,e.movementY*0.12);if(e.movementX||e.movementY)mark(e);}});
 listen(canvas,'wheel',e=>{if(acceptInput()){e.preventDefault();state.scroll(-e.deltaY*0.001);mark(e);}},{passive:false});
 listen(canvas,'contextmenu',e=>e.preventDefault());
 listen(window,'blur',clear);
 listen(document,'visibilitychange',()=>{if(document.hidden)clear();});
 return {state,clear,setMaps:maps=>{state.setMaps(maps);frameSequence=sequence;profile?.clearInput();},takeFrame:()=>{frameSequence=sequence;return state.takeFrame();},consume:step=>profile?.consume(frameSequence,step),dispose:()=>{for(const [n,e,h,o] of listeners)n.removeEventListener(e,h,o);}};
}
