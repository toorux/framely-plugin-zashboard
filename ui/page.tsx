import React from 'react';
import {framely, registerPlugin, Select} from '@framely/sdk';
import css from './style.css';

type Group={name:string;now:string;all:string[];route:string[];exit:string|null;delays?:Record<string,number|null>|null};
type Status={speedTest?:{group:string;total:number;completed:number;running:boolean}|null;ready:boolean;configured:boolean;enabled:boolean;running:boolean;mode:string;name:string;hasSubscription:boolean;updatedAt:number;groups:Group[];primaryGroup:string|null;groupsTruncated:boolean;connections:number;uploadTotal:number;downloadTotal:number;error:string|null;mihomo:string;zashboard:string};
const bytes=(n:number)=>n>=1024**3?`${(n/1024**3).toFixed(1)} GB`:n>=1024**2?`${(n/1024**2).toFixed(1)} MB`:`${(n/1024).toFixed(0)} KB`;
function useService(){
 const [state,setState]=React.useState<Status|null>(null),[error,setError]=React.useState(''),[pollError,setPollError]=React.useState(''),[busy,setBusy]=React.useState(false);
 const live=React.useRef(false),working=React.useRef(false),sample=React.useRef<{time:number;up:number;down:number}|null>(null);
 const [rates,setRates]=React.useState({up:0,down:0});
 function receive(s:Status){const time=performance.now(),last=sample.current;setRates(last&&time-last.time>500&&s.running?{up:Math.max(0,s.uploadTotal-last.up)*1000/(time-last.time),down:Math.max(0,s.downloadTotal-last.down)*1000/(time-last.time)}:{up:0,down:0});sample.current={time,up:s.uploadTotal,down:s.downloadTotal};setState(s);setPollError('');}
 React.useEffect(()=>{live.current=true;let timer:ReturnType<typeof setTimeout>;const poll=async()=>{if(!working.current){try{const s=await framely.call<Status>('status.get');if(live.current)receive(s);}catch(e){if(live.current)setPollError(String(e));}}if(live.current)timer=setTimeout(poll,2000);};void poll();return()=>{live.current=false;clearTimeout(timer);};},[]);
 async function act(fn:()=>Promise<unknown>){if(working.current)return;working.current=true;setBusy(true);setError('');try{await fn();const s=await framely.call<Status>('status.get');if(live.current)receive(s);}catch(e){if(live.current)setError(String(e));}finally{working.current=false;if(live.current)setBusy(false);}}
 const call=(method:string,params:unknown={})=>act(()=>framely.call(method,params));
 return {state,error:error||pollError,busy,call,act,rates};
}
type Service=ReturnType<typeof useService>;
function Header({quick=false,onError}:{quick?:boolean;onError:(e:string)=>void}){
 return <header><div className="brand"><svg width="40" height="40" viewBox="0 0 40 40" aria-hidden="true"><rect x="3" y="3" width="34" height="34" rx="10" fill="none" stroke="currentColor" strokeWidth="2"/><path d="M11 23h5l4-10 4 14 3-7h3" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"/></svg><div><h1>zashboard</h1><span>MIHOMO / ZASHBOARD</span></div></div><div className="header-actions"><button className="close" aria-label="关闭窗口" onClick={()=>void (quick?framely.ui.close():framely.windows.close('settings')).catch(e=>onError(String(e)))}>×</button></div></header>;
}
function Connection({service}:{service:Service}){
 const {state:s,busy,call}=service;if(!s)return null;
 return <section className={'connection '+(s.enabled?'active':'')}><div className="service-heading"><div><span className="status"><i/>{s.enabled?'TUN 代理已开启':s.configured?'TUN 代理已关闭':'尚未添加配置'}</span><p className="profile-name" title={s.name}>{s.configured?s.name:'点击下方「添加配置」开始设置'}</p></div><button className="power" role="switch" aria-label="TUN 代理" aria-checked={s.enabled} disabled={busy||!s.configured} onClick={()=>void call('service.set',{enabled:!s.enabled})}><span className="switch-track" aria-hidden="true"><span className="switch-thumb"/></span></button></div><div className="modes" role="group" aria-label="代理模式">{[['rule','规则'],['global','全局'],['direct','直连']].map(([mode,label])=><button key={mode} aria-pressed={s.mode===mode} disabled={busy||!s.configured} onClick={()=>void call('mode.set',{mode})}>{label}</button>)}</div><p className="mode-note">{!s.enabled?'代理关闭，系统流量保持原有路由。':s.mode==='global'?'公网流量使用 GLOBAL；本地串流保持直连。':s.mode==='direct'?'全部流量直连，不使用代理节点。':'按配置规则分流；本地串流保持直连。'}</p></section>;
}
function Groups({service}:{service:Service}){
 const {state:s,busy,call}=service;
 const [chosen,setChosen]=React.useState('');
 React.useEffect(()=>setChosen(''),[s?.updatedAt]);
 if(!s)return null;
 const groups=s.groups.filter(g=>g.name!=='GLOBAL');
 const group=s.mode==='global'?s.groups.find(g=>g.name==='GLOBAL'):groups.find(g=>g.name===chosen)||groups[0];
 const delays=group?.delays??{};
 const nodes=[...(group?.all??[])].sort((a,b)=>(delays[a]??Infinity)-(delays[b]??Infinity));
 const batch=s.speedTest?.group===group?.name?s.speedTest:null;
 const testing=!!s.speedTest?.running;
 const reachable=nodes.filter(n=>typeof delays[n]==='number');
 const testNote=batch?(batch.running?`正在测速 ${batch.completed}/${batch.total}`:reachable.length?`已测 ${batch.total} 个 · 最低 ${delays[reachable[0]]} ms`:`已测 ${batch.total} 个 · 均不可达 / 超时`):testing?'其他策略组正在测速':'测速后按延迟从低到高排列';
 return <section className="groups"><div className="section-heading"><h2>{s.mode==='global'?'全局出口':'策略与节点'}</h2><span className="muted">{s.mode==='rule'?'按规则分别生效':s.mode==='direct'?'直连模式':'GLOBAL'}</span></div>{s.mode==='direct'?<p className="empty-state">当前使用直连，无需选择节点。</p>:group?<><div className="selectors">{s.mode==='rule'&&<div className="field"><label>策略组</label><Select label="策略组" value={group.name} disabled={busy} options={groups.map(g=>({value:g.name,label:g.name}))} onChange={setChosen}/></div>}<div className="field"><label>{s.mode==='global'?'出口节点':'组内节点'}</label><Select label="组内节点" value={group.now} disabled={busy} options={nodes.map(name=>({value:name,label:name+(Object.hasOwn(delays,name)?(delays[name]===null?' · 不可达':` · ${delays[name]} ms`):'')}))} onChange={name=>void call('proxy.select',{group:group.name,name})}/></div></div><p className="route" title={group.route?.join(' → ')}><span>{s.enabled?'所选出口':'预选出口'}</span><strong>{group.exit??'暂无法解析'}</strong>{group.route?.length>1&&<small>经 {group.route.slice(0,-1).join(' → ')}</small>}</p><div className="node-test"><button disabled={busy||!s.running||testing} onClick={()=>void call('proxy.testAll',{group:group.name})}>{testing?'正在测速…':'测速全部节点'}</button><span role="status">{testNote}</span></div></>:<p className="empty-state">{s.configured?'没有可手动切换的策略组。':'添加配置后可切换策略组和节点。'}</p>}{s.groupsTruncated&&<p className="hint">策略组较多，完整列表请打开 zashboard。</p>}</section>;
}
function Traffic({service}:{service:Service}){
 const {state:s,rates}=service;if(!s)return null;
 return <div className="stats" aria-label="内核实时流量"><div><span>↓ 下载</span><strong>{bytes(rates.down)}<small>/s</small></strong></div><div><span>↑ 上传</span><strong>{bytes(rates.up)}<small>/s</small></strong></div><div><span>当前连接</span><strong>{s.connections}</strong></div></div>;
}
function Actions({service,quick,onConfigure}:{service:Service;quick:boolean;onConfigure:()=>void}){
 const {state:s,busy}=service;
 return <div className="connection-actions"><button className="primary" disabled={busy} onClick={()=>void service.act(()=>framely.windows.open('main'))}>打开 zashboard ↗</button>{quick&&<button disabled={busy} onClick={onConfigure}>{s?.configured?'配置管理':'添加配置'}</button>}{s?.configured&&!s.running&&<button disabled={busy} onClick={()=>void service.call('service.retry')}>重试内核</button>}</div>;
}
function Profile({service,quick=false,onSaved}:{service:Service;quick?:boolean;onSaved?:()=>void}){
 const {state:s,busy,call,act}=service;
 const [tab,setTab]=React.useState('subscription'),[name,setName]=React.useState('我的配置'),[url,setUrl]=React.useState(''),[yaml,setYaml]=React.useState(''),[progress,setProgress]=React.useState(''),[browserUrl,setBrowserUrl]=React.useState('');
 React.useEffect(()=>{if(quick)return;let live=true;void framely.call<{url:string}>('browser.get').then(result=>{if(live)setBrowserUrl(result.url);}).catch(()=>{});return()=>{live=false;};},[quick]);
 async function importText(text:string,filename?:string){await act(async()=>{const label=filename?.replace(/\.(ya?ml|json)$/i,'')||name;await framely.call('profile.begin',{name:label});try{for(let i=0;i<text.length;i+=8000){setProgress(`${Math.min(i+8000,text.length)} / ${text.length}`);await framely.call('profile.chunk',{text:text.slice(i,i+8000)});}await framely.call('profile.commit');setYaml('');onSaved?.();}catch(e){await framely.call('profile.cancel').catch(()=>{});throw e;}finally{setProgress('');}});}
 async function fileChange(e:React.ChangeEvent<HTMLInputElement>){const file=e.target.files?.[0];if(!file)return;e.target.value='';if(file.size>4*1024**2){await act(()=>Promise.reject(new Error('配置文件超过 4 MiB')));return;}try{await importText(await file.text(),file.name);}catch(e){await act(()=>Promise.reject(e));}}
 return <section className="profile"><div className="section-heading"><h2>配置与订阅</h2>{browserUrl&&!(window as any).__framelyBrowser&&<a className="browser-link" href={browserUrl} target="_blank" rel="noopener noreferrer">浏览器配置 ↗</a>}{s?.hasSubscription&&<button disabled={busy} onClick={()=>void call('subscription.update')}>更新订阅</button>}</div>{s?.configured&&<div className="current-profile"><span className="profile-dot"/><div><strong>{s.name}</strong><small>{s.hasSubscription?'HTTPS 订阅':'本地配置'} · {new Date(s.updatedAt*1000).toLocaleString()}</small></div></div>}<div className="tabs" role="tablist">{[['subscription','订阅地址'],['file','导入文件'],['paste','粘贴 YAML']].map(([id,label])=><button key={id} role="tab" aria-selected={tab===id} onClick={()=>setTab(id)} disabled={busy}>{label}</button>)}</div><div className="profile-content" role="tabpanel">{tab==='subscription'?<><label>配置名称<input value={name} disabled={busy} onChange={e=>setName(e.target.value)} maxLength={60}/></label><label>HTTPS 订阅地址<input type="password" autoComplete="off" value={url} disabled={busy} placeholder="https://…" onChange={e=>setUrl(e.target.value)}/></label><p className="hint">支持完整 Clash YAML；地址仅保存在本机。</p><button className="primary" disabled={busy||!url||!name.trim()} onClick={()=>void act(async()=>{await framely.call('subscription.set',{name,url});setUrl('');onSaved?.();})}>导入订阅</button></>:tab==='file'?<><p className="muted">选择 YAML 或 JSON 格式的 Clash 配置，最大 4 MiB。</p><label className={'file-button '+(busy?'disabled':'')}>选择配置文件<input type="file" accept=".yaml,.yml,.json" disabled={busy} onChange={e=>void fileChange(e)}/></label><p className="hint">导入会替换当前配置；校验失败时继续保留原配置。</p></>:<><label>配置名称<input value={name} disabled={busy} onChange={e=>setName(e.target.value)} maxLength={60}/></label><label>Clash YAML<textarea value={yaml} disabled={busy} onChange={e=>setYaml(e.target.value)} rows={4} spellCheck={false} placeholder={'proxies:\n  - name: …\nproxy-groups:\n  …\nrules:\n  …'}/></label><button className="primary" disabled={busy||!yaml||!name.trim()} onClick={()=>void importText(yaml)}>校验并导入</button></>}{progress&&<p role="status">正在导入：{progress}</p>}</div></section>;
}
function App({quick=false}:{quick?:boolean}){
 const service=useService(),[uiError,setUiError]=React.useState(''),[configuring,setConfiguring]=React.useState(false);const s=service.state;
 const error=uiError||service.error||s?.error;
 return <main className={quick?'quick':'manager'}><Header quick={quick} onError={setUiError}/>{error&&<div className="notice" role="alert"><span title={error}>{error}</span>{uiError&&<button aria-label="关闭错误提示" onClick={()=>setUiError('')}>×</button>}</div>}{s?<><div className="content">{quick&&configuring?<div className="inline-config"><button className="back-button" disabled={service.busy} onClick={()=>setConfiguring(false)}>‹ 返回快捷控制</button><Profile service={service} quick onSaved={()=>setConfiguring(false)}/></div>:<><div className="left"><Connection service={service}/><Groups service={service}/><Traffic service={service}/><Actions service={service} quick={quick} onConfigure={()=>setConfiguring(true)}/>{!quick&&<aside>批量测速当前策略组的全部节点，连接详情和日志请打开 zashboard。</aside>}</div>{!quick&&<div className="right"><Profile service={service}/></div>}</>}</div><footer>{!quick&&<><span>Mihomo {s.mihomo} · zashboard {s.zashboard}</span><a href="source.zip" download="zashboard-plugin-source.zip">本版本源码</a></>}<span role="status">{service.busy?'正在处理…':s.running?'内核运行中 · 流量每 2 秒更新':'内核未运行'}</span></footer></>:<p className="loading" role="status">正在连接代理服务…</p>}</main>;
}
function QuickPage(){return <App quick/>;}
function SettingsPage(){return <App/>;}
registerPlugin({QuickPage,windows:{settings:SettingsPage}});
const style=document.createElement('style');style.textContent=css;document.head.append(style);
