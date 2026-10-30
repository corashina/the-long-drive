// One reused 320x180 target. The source thumbnail is a 320x180 JPG.
// A WebGPU canvas is black outside the task that drew it, so the renderer calls frame() after the next presented frame.
export function createThumbnail(canvas){
 const offscreen=typeof OffscreenCanvas==='function',target=offscreen?new OffscreenCanvas(320,180):Object.assign(document.createElement('canvas'),{width:320,height:180}),context=target.getContext('2d');
 const encode=()=>offscreen?target.convertToBlob({type:'image/jpeg',quality:0.9}):new Promise(resolve=>target.toBlob(resolve,'image/jpeg',0.9));
 let request=null,waiting=[];
 return {
  connect(requestFrame){request=requestFrame;},
  frame(){
   const done=waiting;waiting=[];
   let blob;try{context.drawImage(canvas,0,0,320,180);blob=encode().catch(()=>null);}catch{blob=Promise.resolve(null);}
   blob.then(value=>{for(const resolve of done)resolve(value);});
  },
  capture(){if(!request)return Promise.resolve(null);return new Promise(resolve=>{waiting.push(resolve);request();});}
 };
}
