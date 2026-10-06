"""Loopback-only UI preview with synthetic state; no core or system changes."""
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
from pathlib import Path
import argparse

ROOT = Path(__file__).resolve().parent.parent
MOCK = r'''window.__framelyBridge={subscribe:()=>()=>{},request:async(op,p)=>{
if(op==='call'){
 const{method,params}=p;
 if(method==='proxy.testAll'){const group=state.groups.find(g=>g.name===params.group);group.delays={'香港 01':68,'日本 01':35,'新加坡 01':null,DIRECT:120};state.speedTest={group:group.name,total:group.all.length,completed:group.all.length,running:false};return state;}
 if(method==='mode.set')state.mode=params.mode;
 if(method==='service.set')state.enabled=params.enabled;
 if(method==='settings.autoStart')state.autoStart=params.enabled;
 if(method==='proxy.select')state.groups.find(g=>g.name===params.group).now=params.name;
 if(['profile.import','profile.commit','subscription.set'].includes(method)){state.configured=true;state.running=true;state.name=window.draftName||params.name||'预览配置';state.updatedAt=Date.now()/1000;state.hasSubscription=method==='subscription.set';}
 if(method==='profile.begin')window.draftName=params.name;
 for(const g of state.groups){g.route=[g.now];g.exit=g.now;}
 return JSON.parse(JSON.stringify(state));
}
if(op==='window.open'&&p.window==='settings')throw new Error('验证模式：不允许从快捷配置打开独立设置窗口');
if(op==='window.open')window.open(p.window==='main'?'/dashboard':'/settings','_blank');
if(op==='window.close'||op==='ui.close')window.close();
return {};
}};
const state={ready:true,configured:true,enabled:false,autoStart:false,running:true,mode:'rule',name:'我的订阅 · 界面预览',hasSubscription:true,updatedAt:Date.now()/1000,primaryGroup:'代理节点',groups:[{name:'GLOBAL',now:'香港 01',all:['香港 01','日本 01','DIRECT']},{name:'代理节点',now:'香港 01',all:['香港 01','日本 01','新加坡 01','DIRECT']},{name:'流媒体',now:'日本 01',all:['日本 01','香港 01','DIRECT']}],groupsTruncated:false,connections:0,uploadTotal:0,downloadTotal:0,error:null,mihomo:'1.19.32',zashboard:'3.29.1'};
const scenario=new URLSearchParams(location.search).get('scenario');
if(scenario==='empty'){state.configured=false;state.running=false;state.groups=[];state.name='';state.hasSubscription=false;}
if(scenario==='error'){state.running=false;state.error='代理内核已退出，代理已关闭；可重试启动';}
if(scenario==='many'){for(let i=0;i<40;i++)state.groups.push({name:'策略组 '+i,now:'香港 01',all:['香港 01','日本 01','DIRECT']});}
'''
HTML = '''<!doctype html><html lang="zh-CN"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>zashboard preview</title></head><body><div style="font:12px system-ui;color:#a5d4b7;background:#253b30;padding:6px 12px;position:fixed;bottom:0;left:0;z-index:99">界面预览 · 模拟状态 · 未开启系统代理</div><div id="root"></div><script src="/mock.js"></script><script src="/page.js"></script></body></html>'''
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split('?')[0]
        if path in ['/', '/settings', '/quick']:
            data, mime = HTML.encode(), 'text/html; charset=utf-8'
        elif path == '/mock.js':
            data, mime = MOCK.encode(), 'text/javascript'
        elif path == '/page.js':
            data, mime = (ROOT/'payload/page.js').read_bytes(), 'text/javascript'
        elif path == '/source.zip':
            data, mime = (ROOT/'payload/source.zip').read_bytes(), 'application/zip'
        elif path == '/dashboard':
            data, mime = '<!doctype html><meta charset=utf-8><p>此预览仅模拟插件管理页面。完整 zashboard 的实际连接另行验证。</p>'.encode(), 'text/html; charset=utf-8'
        else:
            self.send_error(404); return
        self.send_response(200);self.send_header('Content-Type',mime);self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(data)
    def log_message(self,*args): pass
if __name__ == '__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--port',type=int,default=8788);args=parser.parse_args()
    print(f'Preview: http://127.0.0.1:{args.port}/settings (synthetic UI only)',flush=True)
    ThreadingHTTPServer(('127.0.0.1',args.port),Handler).serve_forever()
