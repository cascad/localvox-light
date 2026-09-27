"""Isolated integration check; no audio, real models, network provider or live archive."""
from pathlib import Path
from http.server import BaseHTTPRequestHandler, HTTPServer
from threading import Thread
from datetime import datetime
import json, os, subprocess, tempfile, time, sys

binary=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parents[1]/'target'/'debug'/('localvox-process.exe' if os.name=='nt' else 'localvox-process')
requests=[]
class Mock(BaseHTTPRequestHandler):
    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(body)
        if len(requests)==1: time.sleep(5.3)  # exercise a heartbeat during blocking HTTP
        if len(requests)>2:
            self.send_response(503); self.end_headers(); self.wfile.write(b'mock unavailable'); return
        answer={'message':{'content':'[1] Сегодня обсудили план работы и подготовку отчета.'},
                'load_duration':1000000,'prompt_eval_duration':2000000,'eval_duration':3000000,'eval_count':12}
        data=json.dumps(answer,ensure_ascii=False).encode()
        self.send_response(200); self.send_header('Content-Type','application/json')
        self.send_header('Content-Length',str(len(data))); self.end_headers(); self.wfile.write(data)
    def log_message(self,*args): pass

server=HTTPServer(('127.0.0.1',0),Mock)
thread=Thread(target=server.serve_forever,daemon=True);thread.start()
try:
    with tempfile.TemporaryDirectory(prefix='localvox-pipeline-smoke-') as temp:
        root=Path(temp); session=root/'sessions'/'fixture'; transcripts=session/'transcripts'
        protocol=subprocess.run([str(binary),'--worker-protocol'],cwd=root,capture_output=True,text=True,timeout=10)
        assert protocol.returncode==0 and protocol.stdout.strip()=='localvox-worker/1', protocol.stderr
        transcripts.mkdir(parents=True); (root/'glossary').mkdir()
        original=json.dumps({'source_id':0,'start_sec':0,'end_sec':8,
            'text':'Сегодня обсудили план работы и подготовку отчета.'},ensure_ascii=False)+'\n'
        source=transcripts/'v001-fixture.jsonl';source.write_text(original,encoding='utf-8')
        (session/'versions.json').write_text(json.dumps({'best':1,'versions':[{'id':1,'label':'fixture',
            'file':source.name,'model':'fixture','created_at':'2026-09-27T00:00:00Z','parents':[]}]}),encoding='utf-8')
        common=[str(binary),str(session),'--post-only','--work-dir',str(root),
            '--model-dir',str(root/'missing-asr-model'),'--glossary-dir',str(root/'glossary'),
            '--llm-base-url',f'http://127.0.0.1:{server.server_port}/:11434/v1','--llm-model','fixture']
        proc=subprocess.run(common+['--refine','--cleanup'],cwd=root,capture_output=True,text=True,encoding='utf-8',timeout=30)
        assert proc.returncode==0, proc.stdout+'\n'+proc.stderr
        events=[json.loads(line) for line in (session/'progress.jsonl').read_text(encoding='utf-8').splitlines()]
        assert all(e.get('stage')!='transcribe' for e in events), 'post-only touched the ASR stage'
        assert (session/'processed.json').exists(), list(session.iterdir())
        assert source.read_text(encoding='utf-8')==original
        refine=[e for e in events if e.get('stage')=='refine']
        assert (datetime.fromisoformat(refine[-1]['at'])-datetime.fromisoformat(refine[0]['at'])).total_seconds()>=5
        assert any('ожидание/работа' in e.get('note','') for e in refine), 'no heartbeat while waiting'
        assert refine[-1]['state']=='done', 'late heartbeat overwrote completion'
        versions=(session/'versions.json').read_text(encoding='utf-8')
        ledger=json.loads((session/'processing.json').read_text(encoding='utf-8'))
        assert all(r.get('receipt',{}).get('outputs') for r in ledger['artifacts'].values())
        # A resumed text phase makes zero extra requests and does not create a new version.
        resumed=subprocess.run(common+['--worker-phase','text','--refine','--cleanup','--summary'],cwd=root,capture_output=True,text=True,encoding='utf-8',timeout=30)
        assert resumed.returncode==0, resumed.stderr
        assert len(requests)==2, 'confirmed work was regenerated'
        assert (session/'versions.json').read_text(encoding='utf-8')==versions
        assert not (session/'summary.md').exists(), 'text phase leaked into summary execution'
        # Removing the output makes the existing receipt invalid. Only cleanup is retried.
        (session/'processed.json').unlink()
        failed=subprocess.run(common+['--cleanup'],cwd=root,capture_output=True,text=True,encoding='utf-8',timeout=30)
        assert failed.returncode==2, failed.stdout+'\n'+failed.stderr
        assert (session/'versions.json').read_text(encoding='utf-8')==versions, 'post failure recooked transcript'
        last=json.loads((session/'progress.jsonl').read_text(encoding='utf-8').splitlines()[-1])
        assert last['stage']=='cleanup' and last['state']=='failed'
        print('PASS: worker protocol; phase isolation; no ASR; heartbeat and terminal ordering; receipts; zero requests on resume; missing-output retry fails visibly without recooking.')
finally:
    server.shutdown();server.server_close();thread.join(timeout=2)
