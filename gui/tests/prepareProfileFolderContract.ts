// Produces synthetic input with the production browser-side folder collector.
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { prepareProfileImport } from '../src/lib/profileWorkspaceBundle.ts';
const destination=resolve(process.argv[2] ?? '../target/profile-folder-contract/input.zip');
const entries: [string,string|Uint8Array][]=[
 ['conf.toml','workers = 1\nflowgraph_dir = "graphs"\n'],
 ['graphs/main.flowgraph.toml','[meta]\ntitle = "Folder import"\n[note]\ntext = ".."\nresource = "assets/payload.bin"\n'],
 ['assets/payload.bin',new Uint8Array([0,255,42,128])],
 ['graphs/.flowgraph-purposes.json',JSON.stringify({schema_version:1,purposes:[{id:'tools',name:'配信支援',roots:['main']}]})],
 ['graphs/.flowgraph-purposes.json.bak','old'],
 ['graphs/credentials/token.txt','synthetic-excluded'],
];
const files=entries.map(([path,contents])=>{
 const file=new File([contents as BlobPart],path.split('/').at(-1)!);
 Object.defineProperty(file,'webkitRelativePath',{value:`workspace/${path}`});
 return file;
});
const bundle=await prepareProfileImport({kind:'workspace',files,configPath:'conf.toml'});
mkdirSync(dirname(destination),{recursive:true});
writeFileSync(destination,new Uint8Array(await bundle.body.arrayBuffer()));
console.log(JSON.stringify({destination,files:bundle.includedFileCount,bytes:bundle.body.size}));
