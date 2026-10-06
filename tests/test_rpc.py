"""Real core integration, with TUN disabled for every test."""
import json
import os
from pathlib import Path
import select
import signal
import ssl
import subprocess
import tempfile
import shutil
import time
import unittest
import urllib.request
import urllib.error
import threading
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler

ROOT = Path(__file__).resolve().parent.parent
PAYLOAD = ROOT / 'payload'
PROFILE = '''proxies:
  - {name: "测试节点 A", type: socks5, server: 127.0.0.1, port: 41081}
  - {name: "测试节点 B", type: socks5, server: 127.0.0.1, port: 41082}
proxy-groups:
  - {name: "选择 / 节点", type: select, proxies: ["测试节点 A", "测试节点 B", DIRECT]}
  - {name: "嵌套策略", type: select, proxies: ["选择 / 节点", DIRECT]}
  - {name: "AAA", type: select, proxies: [DIRECT]}
rules:
  - GEOSITE,cn,DIRECT
  - GEOIP,CN,DIRECT
  - MATCH,嵌套策略
'''

class Session:
    def __init__(self, data, env=None, payload=PAYLOAD):
        self.data = Path(data)
        self.log = (self.data / 'test.stderr').open('ab')
        self.process = subprocess.Popen([str(payload / 'backend')], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, env={**os.environ, 'FRAMELY_DATA_DIR': str(self.data), **(env or {})})
        self.seq = 0

    def call(self, method, params=None, error=False):
        self.seq += 1
        request = {'id': self.seq, 'method': method, 'params': params or {}}
        self.process.stdin.write((json.dumps(request, ensure_ascii=False)+'\n').encode())
        self.process.stdin.flush()
        if not select.select([self.process.stdout], [], [], 14)[0]:
            raise AssertionError('RPC timed out: '+method)
        line = self.process.stdout.readline()
        if not line:
            raise AssertionError('Backend exited: '+(self.data/'test.stderr').read_text())
        response = json.loads(line)
        assert response['id'] == self.seq, response
        if error:
            assert 'error' in response, response
            return response['error']
        if 'error' in response:
            raise AssertionError(response['error']+'\n'+(self.data/'mihomo.log').read_text() if (self.data/'mihomo.log').exists() else response['error'])
        return response['result']

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            self.process.wait(timeout=10)
        self.process.stdout.close()
        if not self.process.stdin.closed:
            self.process.stdin.close()
        self.log.close()

    def killed(self):
        self.process.kill()
        self.process.wait(timeout=5)

def wait_gone(record):
    deadline = time.monotonic()+8
    while time.monotonic() < deadline:
        path = Path(f"/proc/{record['pid']}/exe")
        try:
            if str(path.resolve(strict=True)) != record['executable']:
                return
        except FileNotFoundError:
            return
        time.sleep(.05)
    raise AssertionError('Mihomo remained after backend exit')

class Integration(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='framely-zashboard-test-')
        self.session = Session(self.tmp.name)

    def tearDown(self):
        self.session.close()
        # Standalone recovery also works with no live backend state.
        subprocess.run([str(PAYLOAD/'backend'), '--cleanup'], env={**os.environ,'FRAMELY_DATA_DIR':self.tmp.name}, check=True, timeout=8)
        self.tmp.cleanup()

    def start_profile(self):
        self.session.call('framely.lifecycle.start')
        state = self.session.call('profile.import', {'name':'测试配置', 'yaml':PROFILE})
        self.assertTrue(state['configured'])
        self.assertTrue(state['running'])
        self.assertFalse(state['enabled'])
        runtime = json.loads((Path(self.tmp.name)/'runtime.json').read_text())
        self.assertFalse(runtime['tun']['enable'])
        self.assertEqual(runtime['tun']['device'], 'zashboard-tun')
        self.assertLess(len(runtime['tun']['device'].encode()),16)
        self.assertEqual(runtime['mixed-port'], 0)
        return state, runtime

    def test_lifecycle_validation_selection_and_recovery(self):
        self.assertFalse(self.session.call('status.get')['configured'])
        self.session.call('mode.set', {'mode':'global'}, error=True)
        state, _ = self.start_profile()
        self.assertEqual(state['name'], '测试配置')
        self.assertEqual([g['name'] for g in state['groups'] if g['name']!='GLOBAL'], ['选择 / 节点','嵌套策略','AAA'])
        self.session.call('proxy.testAll',{'group':'不存在'},error=True)
        batch=self.session.call('proxy.testAll',{'group':'选择 / 节点'})
        self.assertEqual(batch['speedTest']['total'],3)
        deadline=time.monotonic()+6
        while batch['speedTest']['running'] and time.monotonic()<deadline:
            time.sleep(.1)
            batch=self.session.call('status.get')
        self.assertFalse(batch['speedTest']['running'])
        measured=next(g for g in batch['groups'] if g['name']=='选择 / 节点')['delays']
        self.assertEqual(set(measured),{'测试节点 A','测试节点 B','DIRECT'})
        self.assertIsNone(measured['测试节点 A'])
        self.assertIsNone(measured['测试节点 B'])
        self.assertFalse(batch['enabled'])
        self.assertEqual(next(g for g in batch['groups'] if g['name']=='选择 / 节点')['now'],'测试节点 A')
        selected = self.session.call('proxy.select', {'group':'选择 / 节点','name':'测试节点 B'})
        self.assertEqual(next(g for g in selected['groups'] if g['name']=='选择 / 节点')['now'], '测试节点 B')
        self.assertEqual(selected['primaryGroup'], '选择 / 节点')
        nested = next(g for g in selected['groups'] if g['name']=='嵌套策略')
        self.assertEqual(nested['route'], ['选择 / 节点', '测试节点 B'])
        self.assertEqual(nested['exit'], '测试节点 B')
        self.assertEqual(self.session.call('mode.set', {'mode':'global'})['mode'], 'global')
        self.session.call('mode.set', {'mode':'bad'}, error=True)
        self.session.call('profile.import', {'name':'broken','yaml':'proxies: []'}, error=True)
        self.assertEqual(self.session.call('status.get')['name'], '测试配置')
        self.session.call('profile.import', {'name':'broken','yaml':'proxies: [{name: bad, type: invalid}]'}, error=True)
        self.assertTrue(self.session.call('status.get')['running'])
        self.assertFalse(json.loads((Path(self.tmp.name)/'runtime.json').read_text())['tun']['enable'])
        record = json.loads((Path(self.tmp.name)/'core.json').read_text())
        self.session.call('framely.lifecycle.stop')
        wait_gone(record)
        self.assertFalse(self.session.call('status.get')['running'])
        self.session.close()
        self.session = Session(self.tmp.name)
        self.session.call('framely.lifecycle.start')
        restored = self.session.call('status.get')
        self.assertEqual(restored['name'], '测试配置')
        self.assertEqual(restored['mode'], 'global')
        self.assertFalse(restored['enabled'])

    def test_window_api_auth_and_host_validation(self):
        _, runtime = self.start_profile()
        window = self.session.call('window.get', {'window':'main'})['url']
        self.assertTrue(window.endswith('/framely-window/main'))
        client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        page = client.open(window).read().decode()
        self.assertIn('disableTunMode=1', page)
        self.assertIn('disableUpgradeCore=1', page)
        for name in ['bridge.js','window.js','relay.js']:
            self.assertEqual(client.open(window.replace('/framely-window/main','/'+name)).status,200)
        with self.assertRaises(urllib.error.HTTPError) as result:
            client.open(urllib.request.Request(window,headers={'Host':'attacker.example'}))
        self.assertEqual(result.exception.code,403)
        with self.assertRaises(urllib.error.HTTPError) as result:
            client.open(urllib.request.Request(window,headers={'Origin':'https://attacker.example'}))
        self.assertEqual(result.exception.code,403)
        base = 'http://'+runtime['external-controller']
        with self.assertRaises(urllib.error.HTTPError) as result:
            client.open(base+'/version')
        self.assertEqual(result.exception.code,401)
        request = urllib.request.Request(base+'/version',headers={'Authorization':'Bearer '+runtime['secret']})
        self.assertEqual(json.load(client.open(request))['version'],'v1.19.32')
        self.assertIn('framely-bridge.js',client.open(base+'/ui/').read().decode())
        self.assertGreater(len(client.open(base+'/ui/framely-bridge.js').read()),1000)
        self.session.call('window.get', {'window':'wrong'}, error=True)

    def test_legacy_loaded_quick_page_window_key_still_declared(self):
        manifest=json.loads((ROOT/'manifest.json').read_text())
        client=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.start_profile()
        # An iframe loaded before an upgrade keeps its old dashboard SDK key.
        for key in ['main','dashboard']:
            self.assertTrue(manifest['ui']['windows'][key]['localWeb'])
            url=self.session.call('window.get',{'window':key})['url']
            self.assertTrue(url.endswith('/framely-window/'+key))
            page=client.open(url).read().decode()
            self.assertIn('data-window="'+key+'"',page)
            self.assertIn('id="dashboard"',page)
            self.assertIn('/window.js',page)
        for key in ['main','dashboard','settings']:
            self.assertTrue(manifest['ui']['windows'][key]['dockIcon'])

    def test_installed_core_permissions_repaired_before_import(self):
        # Match Framely unpack: backend 0755, auxiliary executable 0644.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = root/'payload'
            payload.mkdir()
            for source in PAYLOAD.iterdir():
                if source.name in ['backend','mihomo']:
                    shutil.copy2(source, payload/source.name)
                else:
                    (payload/source.name).symlink_to(source, target_is_directory=source.is_dir())
            (payload/'backend').chmod(0o755)
            (payload/'mihomo').chmod(0o644)
            data = root/'data'
            data.mkdir()
            session = Session(data, payload=payload)
            try:
                session.call('framely.lifecycle.start')
                self.assertEqual((payload/'mihomo').stat().st_mode & 0o777,0o755)
                status = session.call('profile.import',{'name':'安装权限验证','yaml':PROFILE})
                self.assertTrue(status['running'])
                self.assertFalse(status['enabled'])
            finally:
                session.close()

    def test_failed_core_start_reports_stage_without_log_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            payload=root/'payload'
            payload.mkdir()
            for source in PAYLOAD.iterdir():
                if source.name=='backend':
                    shutil.copy2(source,payload/source.name)
                elif source.name!='mihomo':
                    (payload/source.name).symlink_to(source,target_is_directory=source.is_dir())
            # Accept validation, then reproduce a system error during actual start.
            core=payload/'mihomo'
            core.write_text('#!/bin/sh\nif [ "$1" = "-t" ]; then exit 0; fi\necho "level=error permission denied https://example.invalid/secret-subscription" >&2\nexit 1\n')
            core.chmod(0o755)
            data=root/'data'
            data.mkdir()
            session=Session(data,payload=payload)
            try:
                session.call('framely.lifecycle.start')
                error=session.call('profile.import',{'name':'启动失败','yaml':PROFILE},error=True)
                self.assertIn('内核进程在启动期间退出',error)
                self.assertIn('permission denied',error)
                self.assertNotIn('secret-subscription',error)
                state=session.call('status.get')
                self.assertFalse(state['running'])
                self.assertFalse(state['enabled'])
                self.assertFalse((data/'core.json').exists())
            finally:
                session.close()

    def test_browser_configuration_before_core_start(self):
        self.session.call('framely.lifecycle.start')
        url = self.session.call('browser.get')['url']
        base, token = url.split('#')
        origin = base.removesuffix('/settings')
        client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        page = client.open(base).read().decode()
        self.assertIn('/browser.js', page)
        self.assertNotIn(token, page)
        self.assertIn('__framelyBrowser', client.open(origin+'/browser.js').read().decode())
        def rpc(method, params=None, authorization=token, request_origin=origin):
            request = urllib.request.Request(origin+'/rpc', data=json.dumps({'method':method,'params':params or {}}).encode(), headers={'Authorization':'Bearer '+authorization,'Content-Type':'application/json','Origin':request_origin})
            return json.load(client.open(request))
        for authorization, request_origin, code in [('', origin, 401),(token, 'https://attacker.example',403)]:
            with self.assertRaises(urllib.error.HTTPError) as result:
                rpc('status.get',authorization=authorization,request_origin=request_origin)
            self.assertEqual(result.exception.code,code)
        self.assertFalse(rpc('status.get')['result']['configured'])
        empty=client.open(rpc('window.get',{'window':'main'})['result']['url']).read().decode()
        self.assertIn('内核未运行',empty)
        self.assertIn('/window.js',empty)
        with self.assertRaises(urllib.error.HTTPError) as result:
            rpc('framely.lifecycle.stop')
        self.assertEqual(result.exception.code,403)
        rpc('profile.begin',{'name':'浏览器配置'})
        rpc('profile.chunk',{'text':PROFILE})
        status = rpc('profile.commit')['result']
        self.assertTrue(status['configured'])
        self.assertFalse(status['enabled'])
        self.assertEqual(status['name'],'浏览器配置')
        dashboard = rpc('window.get',{'window':'main'})['result']['url']
        page = client.open(dashboard).read().decode()
        self.assertIn('配置管理',page)
        self.assertIn('id="configure"',page)
        self.assertNotIn('target="_blank"',page)

    def test_chunks_and_subscription_errors_keep_profile(self):
        self.session.call('framely.lifecycle.start')
        self.session.call('profile.chunk', {'text':'anything'}, error=True)
        self.session.call('profile.begin', {'name':'分片配置'})
        for offset in range(0,len(PROFILE),20):
            self.session.call('profile.chunk',{'text':PROFILE[offset:offset+20]})
        state = self.session.call('profile.commit')
        self.assertEqual(state['name'],'分片配置')
        self.session.call('subscription.set',{'name':'bad','url':'http://example.org'},error=True)
        self.session.call('subscription.set',{'name':'bad','url':'https://user:password@example.org'},error=True)
        self.session.call('subscription.update',error=True)
        self.assertEqual(self.session.call('status.get')['name'],'分片配置')
        self.session.call('profile.begin',{'name':'cancel'})
        self.session.call('profile.cancel')
        self.session.call('profile.commit',error=True)

    def test_sigkill_backend_stops_real_core(self):
        self.start_profile()
        record = json.loads((Path(self.tmp.name)/'core.json').read_text())
        self.session.killed()
        wait_gone(record)
        deadline = time.monotonic()+3
        while (Path(self.tmp.name)/'core.json').exists() and time.monotonic()<deadline:
            time.sleep(.05)
        self.assertFalse((Path(self.tmp.name)/'core.json').exists())

    def test_eof_and_sigterm_stop_real_core(self):
        self.start_profile()
        record = json.loads((Path(self.tmp.name)/'core.json').read_text())
        self.session.process.send_signal(signal.SIGTERM)
        self.session.process.wait(timeout=10)
        wait_gone(record)

    def test_stale_pid_journal_cannot_kill_unrelated_process(self):
        self.session.close()
        sleeper = subprocess.Popen(['sleep','30'])
        try:
            (Path(self.tmp.name)/'core.json').write_text(json.dumps({'pid':sleeper.pid,'started':'wrong','executable':str(PAYLOAD/'mihomo')}))
            subprocess.run([str(PAYLOAD/'backend'),'--cleanup'],env={**os.environ,'FRAMELY_DATA_DIR':self.tmp.name},check=True,timeout=8)
            self.assertIsNone(sleeper.poll())
        finally:
            sleeper.terminate();sleeper.wait()

    def test_single_backend_and_unexpected_core_exit(self):
        self.start_profile()
        other = Session(self.tmp.name)
        try:
            self.assertNotEqual(other.process.wait(timeout=3),0)
        finally:
            other.close()
        self.assertTrue(self.session.call('status.get')['running'])
        record = json.loads((Path(self.tmp.name)/'core.json').read_text())
        os.kill(record['pid'],signal.SIGTERM)
        wait_gone(record)
        deadline = time.monotonic()+3
        while time.monotonic()<deadline:
            state = self.session.call('status.get')
            if not state['running']:
                break
            time.sleep(.05)
        self.assertFalse(state['enabled'])
        self.assertFalse(state['running'])
        self.assertTrue(state['error'])
        self.assertTrue(self.session.call('service.retry')['running'])

    def test_https_subscription_import_update_and_downgrade_rejection(self):
        self.session.close()
        cert, key = Path(self.tmp.name)/'cert.pem', Path(self.tmp.name)/'key.pem'
        ca, ca_key, csr = Path(self.tmp.name)/'ca.pem', Path(self.tmp.name)/'ca-key.pem', Path(self.tmp.name)/'server.csr'
        extensions=Path(self.tmp.name)/'server.ext'
        extensions.write_text('subjectAltName=DNS:localhost\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n')
        for command in [
            ['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(ca_key),'-out',str(ca),'-days','1','-subj','/CN=zashboard Test CA'],
            ['openssl','req','-new','-newkey','rsa:2048','-nodes','-keyout',str(key),'-out',str(csr),'-subj','/CN=localhost'],
            ['openssl','x509','-req','-in',str(csr),'-CA',str(ca),'-CAkey',str(ca_key),'-CAcreateserial','-out',str(cert),'-days','1','-extfile',str(extensions)],
        ]:
            subprocess.run(command,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        hits=[]
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                hits.append(self.path)
                if self.path=='/redirect':
                    self.send_response(302);self.send_header('Location','http://localhost/profile');self.end_headers();return
                if self.path=='/large':
                    content=b'x'*(4*1024*1024+1)
                elif self.path=='/invalid':
                    content=b'proxies: []'
                else:
                    content=PROFILE.encode()
                self.send_response(200);self.send_header('Content-Length',str(len(content)));self.end_headers();self.wfile.write(content)
            def log_message(self,*args):pass
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(cert,key)
        server.socket=context.wrap_socket(server.socket,server_side=True)
        worker=threading.Thread(target=server.serve_forever,daemon=True);worker.start()
        self.session=Session(self.tmp.name,{'SSL_CERT_FILE':str(ca)})
        try:
            self.session.call('framely.lifecycle.start')
            base=f'https://localhost:{server.server_port}'
            state=self.session.call('subscription.set',{'name':'本地 HTTPS 测试订阅','url':base+'/profile?token=fixture'})
            self.assertTrue(state['hasSubscription'])
            self.assertNotIn('token=fixture',json.dumps(state))
            state=self.session.call('subscription.update')
            self.assertEqual(hits.count('/profile?token=fixture'),2)
            for path in ['/redirect','/invalid','/large']:
                self.session.call('subscription.set',{'name':'invalid','url':base+path},error=True)
                self.assertEqual(self.session.call('status.get')['name'],'本地 HTTPS 测试订阅')
        finally:
            server.shutdown();server.server_close();worker.join(timeout=3)
