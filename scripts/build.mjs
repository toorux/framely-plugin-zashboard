import {build} from 'esbuild';
import {mkdir,copyFile,chmod,rename} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
await mkdir('payload',{recursive:true});
if(!process.argv.includes('--ui-only')){
 if(process.arch!=='arm64'||process.platform!=='linux')throw Error('Build the offline plugin on ARM64 Linux');
 execFileSync('python3',['scripts/upstream.py'],{stdio:'inherit'});
 execFileSync('python3',['scripts/prepare.py'],{stdio:'inherit'});
 execFileSync('sh',['scripts/cargo.sh','build','--release','--locked','--manifest-path','backend/Cargo.toml'],{stdio:'inherit'});
 await copyFile('backend/target/release/framely-zashboard','payload/.backend.tmp');await chmod('payload/.backend.tmp',0o755);await rename('payload/.backend.tmp','payload/backend');
}
await build({entryPoints:['ui/page.tsx'],outfile:'payload/page.js',bundle:true,minify:true,loader:{'.css':'text'},define:{'process.env.NODE_ENV':'"production"'}});
await build({entryPoints:['ui/window.ts'],outfile:'payload/window.js',bundle:true,minify:true,define:{'process.env.NODE_ENV':'"production"'}});
await build({entryPoints:['vendor/framely-sdk/src/bootstrap.ts'],outfile:'payload/bridge.js',bundle:true,minify:true});
await mkdir('payload/dashboard',{recursive:true});
await copyFile('payload/bridge.js','payload/dashboard/framely-bridge.js');
if(!process.argv.includes('--ui-only')){
 for(const [from,to] of [['icon.png','icon.png'],['LICENSE','LICENSE'],['README.md','README.md'],['THIRD_PARTY_NOTICES.md','THIRD_PARTY_NOTICES.md'],['GEO_DATA_NOTICES.md','GEO_DATA_NOTICES.md'],['upstream.lock.json','upstream.lock.json'],['vendor/framely-sdk/LICENSE','LICENSE.framely-sdk'],['node_modules/react/LICENSE','LICENSE.react'],['node_modules/react-dom/LICENSE','LICENSE.react-dom']])await copyFile(from,`payload/${to}`);
 execFileSync('python3',['scripts/source.py'],{stdio:'inherit'});
}
console.log('Built plugin payload; no TUN or system proxy was enabled.');
