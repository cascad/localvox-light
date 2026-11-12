// Fictional, coherent demonstration data. Shared by the real UI and TUI captures.
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../../', import.meta.url));
const original = JSON.parse(await readFile(path.join(root, 'tests/fixtures/workspace.json'), 'utf8'));
const clock = new Date().toISOString();
const lines = [
  ['Аня', 'Давайте зафиксируем, что войдёт в ближайший выпуск. Запись, расшифровка и удобная работа с готовым текстом.'],
  ['Миша', 'Главное — видеть ход обработки: что уже закончилось, что выполняется сейчас и сколько времени это занимает.'],
  ['Лена', 'И чтобы к встрече можно было вернуться из терминала. Выбрал запись, открыл сводку — всё под рукой.'],
  ['Аня', 'Навигацию оставляем привычной: j и k для списка, Enter или l открывает запись, Esc возвращает на уровень выше.'],
  ['Миша', 'Поиск по названию нужен прямо в списке. Нажал слеш, ввёл пару слов и сразу видишь подходящие встречи.'],
  ['Лена', 'Готовую часть длинной записи можно читать ещё до Stop. Новый текст должен добавляться без прыжков прокрутки.'],
  ['Аня', 'Полный текст копируется одним сочетанием. А в репликах и сводке можно взять только выбранный фрагмент.'],
  ['Миша', 'Сводка остаётся опциональной. Для локальной обработки модель выбирается в настройках, там же адрес Ollama.'],
  ['Лена', 'На следующей проверке я пройду сценарий на Mac: демон, разрешения, микрофон и системный звук.'],
  ['Аня', 'Сборки должны приезжать одним комплектом: демон, обработчик и оба интерфейса. Без ручного поиска библиотек.'],
  ['Миша', 'Договорились. После выпуска собираем конкретные баги и исправляем их небольшими проверяемыми изменениями.'],
  ['Лена', 'Я записала решения и следующие шаги. Можно возвращаться к работе.'],
].map(([who,text], i) => ({id:`demo-line-${i}`,who,text,source_id:i%3===0?0:1,start_sec:i*118,end_sec:(i+1)*118-5}));
const main = '20261111_release';
const names = ['План выпуска · LocalVox','Архитектура фоновой обработки','Интервью · исследование продукта','Еженедельная встреча команды','Обсуждение интерфейса','Подкаст · локальные модели','Планирование следующей недели','Заметки после демо'];
const sessions = names.map((title,i)=>({...original.sessions[0],name:i?`202611${11-i}_demo`:main,title,
  started_at:`2026-11-${String(11-i).padStart(2,'0')}T10:00:00+03:00`,duration_sec:i?1680+i*137:1440,
  source_url:null,job:i===1?'running':i===2?'pending':null,cooked:i!==1&&i!==2,has_processed:true,has_summary:i!==1&&i!==2}));
const block=(kind,text,from=0,to=from)=>({kind,text,sources:kind==='heading'?[]:[{line:from,start_sec:lines[from].start_sec,end_sec:lines[to].end_sec}]});
const summary={available:true,blocks:[
 block('heading','План выпуска'),
 block('paragraph','Команда согласовала ближайший выпуск: запись встреч, ранняя расшифровка и два интерфейса для ежедневной работы.',0,2),
 block('heading','Решения'),
 block('bullet','Показывать готовый текст во время записи, сохраняя прокрутку и выделение.',5),
 block('bullet','Сделать одинаковые сценарии чтения и копирования в браузере и терминале.',3,6),
 block('bullet','Оставить сводку опциональной; модель и сервер выбирать в настройках.',7),
 block('heading','Следующие шаги'),
 block('bullet','Лена — проверить демон, разрешения и звук на Mac.',8),
 block('bullet','Миша — проверить установку полного пакета на Windows и macOS.',9),
 block('bullet','Аня — собрать обратную связь после выпуска.',10),
]};
const at=offset=>new Date(Date.parse(clock)+offset*1000).toISOString();
const event=(id,stage,kind,label,offset,extra={})=>({id,run:'demo-run',stage,kind,label,at:at(offset),...extra});
const journal={...original.journal,reset:false,next_cursor:'promo:1',events:[
 event('capture-start','capture','stage','Запись',-1542,{status:'running'}),
 event('s1','capture','stage','Запись завершена',-102,{status:'done',elapsed_sec:1440}),
 event('s2','transcribe','stage','transcribe',-101,{status:'running'}),
 event('o1','transcribe','operation_start','Распознавание готовой части',-100,{operation:'asr',command:{program:'localvox-process',args:[{name:'--engine',value:'gigaam'},{name:'--lang',value:'ru'},{name:'--worker-phase',value:'transcribe'}]}}),
 ...Array.from({length:40},(_,i)=>event(`l${i}`,'transcribe','log','Окно готово',-98+i,{operation:'asr',text:`Источник ${i%2} · окно ${i+1}/40 · результат сохранён`})),
 event('o2','transcribe','operation_end','Распознавание готовой части',-56,{operation:'asr',elapsed_sec:44.2}),
 event('s3','transcribe','stage','transcribe',-56,{status:'done',elapsed_sec:44.2}),
 event('sk1','refine','stage','refine',-56,{status:'skipped',text:'Выключено в настройках'}),
 event('sk2','cleanup','stage','cleanup',-56,{status:'skipped',text:'Используется исходная расшифровка'}),
 event('s4','summary','stage','summary',-54,{status:'running'}),
 event('o3','summary','operation_start','Сводка встречи',-54,{operation:'summary',command:{program:'ollama',args:[{name:'model',value:'qwen3.5:9b'},{name:'temperature',value:'0'}]}}),
 event('s5','summary','progress','Проверка ссылок на источник',-2,{operation:'summary',done:3,total:4}),
]};
const progress={running:true,phase:'summary',stages:[
 {stage:'transcribe',state:'done',started_at:at(-101),ended_at:at(-56),done:40,total:40},
 {stage:'refine',state:'skipped',note:'Выключено в настройках'},
 {stage:'cleanup',state:'skipped',note:'Используется исходная расшифровка'},
 {stage:'summary',state:'running',started_at:at(-54),updated_at:at(-2),done:3,total:4,note:'Проверка ссылок на источник'}],
 execution:{run:'demo-run',state:'running',current:'summary',phase:'summary',since:at(-54),last_signal_at:at(-2),operation:'Сводка встречи',next:[],message:'Готовится сводка'},
 timing:{queued_at:at(-102.2),started_at:at(-101),queue_sec:1.2,processing_sec:101,total_sec:102.2}};
const settings={path:'LocalVox/.env',defaults_path:'LocalVox/.env.defaults',exists:true,extra:[],settings:[
 {key:'LOCALVOX_LANG',group:'Распознавание',label:'Язык новых записей',kind:'choice',options:['ru','en','auto'],value:'ru',default_value:'ru',set:true,applies:'restart',hint:'Язык новых записей и исходной расшифровки.'},
 {key:'LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE',group:'Автоварка',label:'Распознавать во время записи',kind:'bool',value:null,default_value:'on',set:false,applies:'live',hint:'Готовые фрагменты можно читать до остановки записи.'},
 {key:'LOCALVOX_LIGHT_AUTOCOOK_SUMMARY',group:'После распознавания',label:'Сводка · summary',kind:'bool',value:null,default_value:'on',set:false,applies:'live',hint:'Краткие решения и следующие шаги со ссылками на исходную речь.'},
 {key:'LOCALVOX_LLM_MODEL',group:'LLM',label:'Модель',kind:'text',value:'qwen3.5:9b',default_value:'qwen3.5:9b',set:true,applies:'restart',hint:'Локальная модель для сводки.'},
 {key:'LOCALVOX_LLM_BASE_URL',group:'LLM',label:'Адрес сервера',kind:'text',value:'http://localhost:11434/v1',default_value:'http://localhost:11434/v1',options:['http://localhost:11434/v1'],set:true,applies:'restart',hint:'Сервер Ollama или совместимый API.'},
]};

export async function startFixture() {
 let audio;
 const server=createServer(async(req,res)=>{
  try {
   const url=new URL(req.url,'http://localhost');const p=url.pathname;
   res.setHeader('Cache-Control','no-store');
   if(p.startsWith('/api/')) {
    let body={};
    if(p==='/api/health') body={ok:true,service:'localvox-light',version:'0.1.0',api_protocol:1,capabilities:['technical-journal-v1','terminal-workspace-v1']};
    else if(p==='/api/sessions') body=sessions;
    else if(p==='/api/record') body={...original.record,recording:false,session:null};
    else if(p==='/api/jobs') body={running:1,pending:1,failed:0,cancelling:0,cancelled:0,current:sessions[1].name};
    else if(p==='/api/queue') body={queue:[{...original.queue[0],session:sessions[1].name,title:names[1],kind:'cook',phase:'summary',state:'running',position:null},{session:sessions[2].name,title:names[2],kind:'cook',phase:'transcribe',state:'pending',position:1}]};
    else if(p.endsWith('/summary')) body=summary;
    else if(p.endsWith('/transcript')) body={lines};
    else if(p.endsWith('/processed')) body={lines,version_id:1,from_transcript:true,omitted:0,rejected:0};
    else if(p.endsWith('/versions')) body={versions:[{...original.versions[0],model:'gigaam-v3',lines:lines.length}]};
    else if(p.endsWith('/speakers')) body={speakers:['Аня','Миша','Лена'].map((name,i)=>({name,what:'Участник встречи',named:true,voices:[`speaker_${i}`],sources:[i%2],speech_sec:340+i*37,lines:4}))};
    else if(p.endsWith('/sources')) body={sources:[{source_id:0,name:'Микрофон',custom:true,what:'локальная дорожка'},{source_id:1,name:'Команда',custom:true,what:'системный звук'}]};
    else if(p.endsWith('/journal')) body=journal;
    else if(p.endsWith('/progress')) body=progress;
    else if(p.endsWith('/playback')) body={duration_sec:1440,snapshot:null,partial:null};
    else if(p.endsWith('/peaks')) body={duration_sec:1440,peaks:Array.from({length:480},(_,i)=>.09+.65*Math.abs(Math.sin(i*.17)*Math.cos(i*.031)))};
    else if(p==='/api/settings') body=settings;
    else if(p==='/api/slots/notes') body={slots:original.slots};
    else if(p==='/api/slots') body={slots:['идеи']};
    else if(p==='/api/asks') body=original.asks;
    else if(p==='/api/asks/demo-ask') body=original.ask;
    else if(p==='/api/langs') body={langs:['ru','en']};
    else if(p.endsWith('/audio.wav')) {
     if(!audio) {
      const bytes=1440*16000*2;audio=Buffer.alloc(44+bytes);
      audio.write('RIFF',0);audio.writeUInt32LE(36+bytes,4);audio.write('WAVEfmt ',8);
      audio.writeUInt32LE(16,16);audio.writeUInt16LE(1,20);audio.writeUInt16LE(1,22);audio.writeUInt32LE(16000,24);audio.writeUInt32LE(32000,28);audio.writeUInt16LE(2,32);audio.writeUInt16LE(16,34);audio.write('data',36);audio.writeUInt32LE(bytes,40);
     }
     const range=/bytes=(\d+)-(\d*)/.exec(req.headers.range??'');const start=range?Number(range[1]):0;const end=range&&range[2]?Math.min(Number(range[2]),audio.length-1):audio.length-1;
     res.setHeader('Content-Type','audio/wav');res.setHeader('Accept-Ranges','bytes');
     if(range) {res.statusCode=206;res.setHeader('Content-Range',`bytes ${start}-${end}/${audio.length}`);}
     res.setHeader('Content-Length',end-start+1);res.end(audio.subarray(start,end+1));return;
    }
    else if(p==='/api/search') body={results:[{session:main,title:names[0],text:lines[3].text,line:3,start_sec:354}]};
    else if(req.method!=='GET') body={msg:'Сохранено',need_restart:[]};
    res.setHeader('Content-Type','application/json; charset=utf-8');res.end(JSON.stringify(body));return;
   }
   const confined=(base,relative)=>{
    const file=path.resolve(base,relative);
    if(!file.startsWith(path.resolve(base)+path.sep)) throw new Error('Outside static root');
    return file;
   };
   let file;
   if(p==='/terminal') file=path.join(root,'scripts/promo/terminal.html');
   else if(p.startsWith('/xterm/')) file=confined(path.join(root,'ui/node_modules/@xterm/xterm'),decodeURIComponent(p.slice(7)));
   else if(p.startsWith('/fonts/')) file=path.join(root,'ui/node_modules/@fontsource/ibm-plex-mono/files',path.basename(p));
   else file=confined(path.join(root,'ui/dist'),p==='/'?'index.html':decodeURIComponent(p.slice(1)));
   const ext=path.extname(file);res.setHeader('Content-Type',({'.js':'text/javascript','.css':'text/css','.html':'text/html','.woff2':'font/woff2','.svg':'image/svg+xml'})[ext]??'application/octet-stream');
   res.end(await readFile(file));
  } catch {res.writeHead(404);res.end();}
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 return {address:`http://127.0.0.1:${server.address().port}`,close:()=>new Promise(resolve=>server.close(resolve))};
}

export {clock,main};
