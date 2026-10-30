import {readPackageAsset,assetReceipt,checkImageSize} from './world-assets.js';
export function layoutBox(layout,width,height) {
 const c=layout.canvas,[rw,rh]=c.resolution;
 const x=width/rw,y=height/rh;
 const scale=c.mode===0?1:c.screen_match===1?Math.min(x,y):c.screen_match===2?Math.max(x,y):Math.pow(x,1-c.match)*Math.pow(y,c.match);
 let box=[0,0,width/scale,height/scale],fontScale=scale;
 for(const r of layout.ancestry){
  const [px,py,pw,ph]=box,a=r.anchor_min,b=r.anchor_max,p=r.pivot;
  const w=(pw*(b[0]-a[0])+r.size[0])*Math.abs(r.scale[0]),h=(ph*(b[1]-a[1])+r.size[1])*Math.abs(r.scale[1]);
  box=[px+pw*(a[0]+(b[0]-a[0])*p[0])+r.position[0]-w*p[0],py+ph*(a[1]+(b[1]-a[1])*p[1])+r.position[1]-h*p[1],w,h];
  fontScale*=Math.abs(r.scale[1]);
 }
 return {x:box[0]*scale,y:box[1]*scale,width:box[2]*scale,height:box[3]*scale,fontSize:layout.font_size*fontScale,scale};
}
export function applyLayout(element,layout,urls){
 const b=layoutBox(layout,innerWidth,innerHeight),s=element.style;
 s.left=b.x+'px';s.top=b.y+'px';s.width=b.width+'px';s.height=b.height+'px';
 s.fontFamily=layout.font;s.fontSize=b.fontSize+'px';s.color=rgba(layout.color);
 if(layout.sprite!==null && urls){s.backgroundImage='url("'+urls[layout.sprite]+'")';s.backgroundSize='100% 100%';s.backgroundColor=rgba(layout.image_color);}
 return b;
}
export function rgba(c){return 'rgba('+Math.round(c[0]*255)+','+Math.round(c[1]*255)+','+Math.round(c[2]*255)+','+c[3]+')';}
export async function loadUiAssets(ui) {
 const urls=await Promise.all(ui.sprites.map(async sprite=>{
  const bytes=await readPackageAsset(sprite.path);
  const url=URL.createObjectURL(new Blob([bytes],{type:'image/png'}));
  const image=new Image();image.src=url;await image.decode();
  checkImageSize(assetReceipt(sprite.path),image.naturalWidth,image.naturalHeight);
  return url;
 }));
 await document.fonts.load('16px "Liberation Sans"');
 const tinted=new Map();
 async function tint(index,color){
  const key=index+':'+color.join(',');if(tinted.has(key))return tinted.get(key);
  const bitmap=await createImageBitmap(await (await fetch(urls[index])).blob());
  const canvas=document.createElement('canvas');canvas.width=bitmap.width;canvas.height=bitmap.height;
  const ctx=canvas.getContext('2d');ctx.drawImage(bitmap,0,0);bitmap.close();
  const pixels=ctx.getImageData(0,0,canvas.width,canvas.height);
  for(let i=0;i<pixels.data.length;i+=4)for(let c=0;c<4;c++)pixels.data[i+c]=Math.round(pixels.data[i+c]*color[c]);
  ctx.putImageData(pixels,0,0);const blob=await new Promise(resolve=>canvas.toBlob(resolve));
  const result=URL.createObjectURL(blob);tinted.set(key,result);return result;
 }
 ui.pairUrls=[];for(const p of ui.pairs)ui.pairUrls.push({background:await tint(p.background,ui.overlay_color),frame:await tint(p.frame,ui.frame_color)});
 ui.layoutUrls={};for(const [role,e] of Object.entries(ui.layouts))if(e.sprite!==null)ui.layoutUrls[role]=await tint(e.sprite,e.image_color);
 ui.urls=urls;return urls;
}
