// Value 0 is Auto: Rust steps the scale on the F3 GPU time.
export function installSceneResolution(target,select){
 let percent=100;
 select.value='100';
 select.addEventListener('change',()=>{percent=Number(select.value);});
 target.gameSceneScale=()=>percent;
}
