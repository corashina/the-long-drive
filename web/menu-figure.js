// Start menu figure (`menuIkScript`): Rust reads the mouse once per frame while the start menu is open.
// The menu covers the canvas, so the window listens. Unity `Input` ignores the UI, so clicks on buttons count too.
const BUTTONS={0:1,2:2};
export function createMenuFigure({target,canvas}){
 let open=false,x=.5,y=.5,buttons=0,pressed=0;
 function move(event){
  const r=canvas.getBoundingClientRect();if(!r.width||!r.height)return;
  x=(event.clientX-r.left)/r.width;y=(event.clientY-r.top)/r.height;
 }
 target.addEventListener('pointermove',move);
 target.addEventListener('pointerdown',event=>{move(event);const bit=BUTTONS[event.button];if(bit){buttons|=bit;pressed|=bit;}});
 target.addEventListener('pointerup',event=>{const bit=BUTTONS[event.button];if(bit)buttons&=~bit;});
 target.addEventListener('blur',()=>{buttons=0;});
 // The right button drags the figure's leg, as in the source menu.
 target.addEventListener('contextmenu',event=>{if(open&&!event.target.closest?.('input,select,textarea'))event.preventDefault();});
 target.gameMenuFigure=out=>{if(!open)return false;out[0]=x;out[1]=y;out[2]=buttons;out[3]=pressed;pressed=0;return true;};
 return {
  get isOpen(){return open;},
  toggle(shown){if(open===shown)return;open=shown;buttons=0;pressed=0;},
 };
}
