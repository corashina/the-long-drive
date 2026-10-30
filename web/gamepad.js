// Raw pad state for Rust `GamepadFrame`: 4 pads, 8 axes and 32 button bits per pad. No mapping here.
export const PADS=4,AXES=8,BUTTONS=32;
// `getGamepads` gives the `navigator.getGamepads()` list. Missing pads read as zero.
export function createGamepad(getGamepads){
 const axes=new Float32Array(PADS*AXES),buttons=new Uint32Array(PADS);
 return {axes,buttons,poll(){
  axes.fill(0);buttons.fill(0);
  let pads;try{pads=getGamepads()??[];}catch{pads=[];}
  for(let p=0;p<PADS;p++){
   const pad=pads[p];if(!pad?.connected)continue;
   for(let a=0;a<Math.min(AXES,pad.axes.length);a++)axes[p*AXES+a]=pad.axes[a];
   let bits=0;for(let b=0;b<Math.min(BUTTONS,pad.buttons.length);b++)if(pad.buttons[b].pressed)bits|=1<<b;
   buttons[p]=bits>>>0;
  }
 }};
}
