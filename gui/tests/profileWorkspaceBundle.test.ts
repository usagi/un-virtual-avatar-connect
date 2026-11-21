import assert from 'node:assert/strict';
import test from 'node:test';
import { unzipSync, strFromU8 } from 'fflate';
import { prepareProfileImport } from '../src/lib/profileWorkspaceBundle.ts';
function file(path: string, text: string): File {
 const result=new File([text],path.split('/').at(-1)!);
 Object.defineProperty(result,'webkitRelativePath',{value:`workspace/${path}`});
 return result;
}
async function unpack(files: File[]) {
 const prepared=await prepareProfileImport({kind:'workspace',files,configPath:'conf.toml'});
 return unzipSync(new Uint8Array(await prepared.body.arrayBuffer()));
}
test('literal strings in config do not abort folder import',async()=>{
 const archive=await unpack([file('conf.toml','label = "."\nflowgraph_dir = "graphs"'),file('graphs/main.toml','name = "main"')]);
 assert.deepEqual(Object.keys(archive).sort(),['conf.toml','graphs/main.toml']);
});
test('non-path strings in graph do not hide subsequent material references',async()=>{
 const archive=await unpack([file('conf.toml','flowgraph_dir = "graphs"'),file('graphs/main.toml','text = ".."\nresource = "assets/text.txt"'),file('assets/text.txt','keep me')]);
 assert.equal(strFromU8(archive['assets/text.txt']),'keep me');
});
test('invalid file paths are rejected and credentials are excluded',async()=>{
 await assert.rejects(unpack([file('conf.toml','workers = 1'),file('../escape.txt','no')]),/不正/);
 const archive=await unpack([file('conf.toml','secret_file = "credentials/token.txt"\nresource = "assets/text.txt"'),file('credentials/token.txt','private'),file('assets/text.txt','public')]);
 assert.equal(archive['credentials/token.txt'],undefined);
 assert.equal(strFromU8(archive['assets/text.txt']),'public');
});

test('folder migration retains current purpose registry without private history or nested credentials',async()=>{
 const current='{"purposes":[{"name":"My alerts"}]}';
 const archive=await unpack([
  file('conf.toml','flowgraph_dir = "graphs"'),
  file('graphs/main.toml','name = "main"'),
  file('graphs/.flowgraph-purposes.json',current),
  file('graphs/.flowgraph-purposes.json.bak','old registry'),
  file('graphs/main.toml.bak','old graph'),
  file('graphs/.flowgraph-import-journal.json','incomplete import'),
  file('graphs/credentials/token.txt','private'),
  file('graphs/tmp.local/work.txt','temporary'),
  file('graphs/flowgraph.disabled/old.toml','name = "disabled"'),
 ]);
 assert.deepEqual(Object.keys(archive).sort(),['conf.toml','graphs/.flowgraph-purposes.json','graphs/main.toml']);
 assert.equal(strFromU8(archive['graphs/.flowgraph-purposes.json']),current);
});

test('folder migration excludes hidden and backup directory contents like native export',async()=>{
 const archive=await unpack([
  file('conf.toml','flowgraph_dir = "graphs"'),
  file('graphs/main.toml','name = "main"'),
  file('graphs/.git/config','private history'),
  file('graphs/archive.bak/old.toml','old configuration'),
  file('graphs/.flowgraph-purposes.json','{"purposes":[]}'),
 ]);
 assert.deepEqual(Object.keys(archive).sort(),['conf.toml','graphs/.flowgraph-purposes.json','graphs/main.toml']);
});
