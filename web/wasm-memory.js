// One check of the total WASM memory: the main page and every worker.
const MIB=1048576;
// Half the device memory when the browser gives it, else 4 GiB.
export function wasmMemoryLimit(deviceMemory){
 return (Number.isFinite(deviceMemory)&&deviceMemory>0?deviceMemory/2:4)*1024*MIB;
}
// The failure message when `total` bytes are above `limit`, else null.
export function wasmMemoryFailure(total,limit){
 if(total<=limit)return null;
 return `WASM memory ${Math.round(total/MIB)} MiB is above the ${Math.round(limit/MIB)} MiB limit of this device. Lower the draw distance.`;
}
