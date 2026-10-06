import {framely} from '@framely/sdk';

// Native localWeb pages already have the SDK bootstrap. Standalone local
// browsers use the authenticated browser adapter instead of a native window.
async function setup(){
 if(window.parent===window){
  await new Promise<void>((resolve,reject)=>{const script=document.createElement('script');script.src='/browser.js';script.onload=()=>resolve();script.onerror=()=>reject(new Error('无法加载浏览器适配'));document.head.append(script);});
 }
 const key=document.body.dataset.window??'main';
 const notice=document.getElementById('window-error')!;
 async function act(action:()=>Promise<unknown>){try{notice.hidden=true;await action();}catch(e){notice.textContent=String(e);notice.hidden=false;}}
 document.getElementById('configure')!.onclick=()=>void act(()=>framely.windows.open('settings'));
 document.getElementById('close')!.onclick=()=>void act(()=>framely.windows.close(key));
 document.getElementById('refresh')!.onclick=()=>location.reload();
}
void setup();
