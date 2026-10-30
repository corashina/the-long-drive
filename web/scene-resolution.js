export function installSceneResolution(target,select){
 let percent=100;
 select.value='100';
 select.addEventListener('change',()=>{percent=Number(select.value);});
 target.gameSceneScale=()=>percent;
}
