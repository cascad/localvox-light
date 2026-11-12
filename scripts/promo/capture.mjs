// npm --prefix ui ci && npm --prefix ui run build
// cargo build -p localvox-light-terminal --example promo-capture --jobs 2
// node scripts/promo/capture.mjs [--gui-only|--tui-only]
import { mkdir, mkdtemp, readFile, readdir, writeFile, stat, rm } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { startFixture, clock } from './fixture.mjs';
const root=fileURLToPath(new URL('../../',import.meta.url));
const require=createRequire(path.join(root,'ui/package.json'));
const {chromium}=require('@playwright/test');
const out=path.join(root,'docs/media');const work=path.join(root,'target/promo');
await mkdir(out,{recursive:true});await mkdir(work,{recursive:true});
const run=(exe,args,options={})=>new Promise((resolve,reject)=>{const child=spawn(exe,args,{cwd:root,stdio:'inherit',windowsHide:true,timeout:120000,...options});child.on('error',reject);child.on('close',code=>code===0?resolve():reject(new Error(`${exe} exited ${code}`)));});
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const fixture=await startFixture();const browser=await chromium.launch({headless:true});
const context=await browser.newContext({viewport:{width:1440,height:930},locale:'ru-RU',timezoneId:'Europe/Moscow',colorScheme:'dark',reducedMotion:'reduce'});
const outputs=[];
async function image(page,name){
 if(name.startsWith('gui-')&&await page.locator('.app').evaluate(el=>getComputedStyle(el).display)!=='grid') throw new Error('GUI stylesheet not active at '+name);
 await page.screenshot({path:path.join(out,name)});outputs.push(name);console.log('Captured',name);
}
async function gif(folder,name,fps=5){
 await run('ffmpeg',['-hide_banner','-loglevel','error','-y','-framerate',String(fps),'-i',path.join(folder,'%04d.png'),'-filter_complex','[0:v]split[a][b];[a]palettegen=stats_mode=diff:max_colors=128[p];[b][p]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle','-loop','0',path.join(out,name)]);
 outputs.push(name);
 await rm(folder,{recursive:true}); // A unique, capture-owned directory under target/promo.
}
async function gui(){
 const page=await context.newPage();await page.clock.install({time:new Date(clock)});
 const showTab=async name=>{
  await page.getByRole('button',{name,exact:true}).click();
  if(name==='Сводка') await page.getByText('Команда согласовала ближайший выпуск:',{exact:false}).waitFor();
  else if(name==='Реплики'||name==='Текст') await page.getByText('Давайте зафиксируем, что войдёт в ближайший выпуск.',{exact:false}).first().waitFor();
  else if(name==='Участники') await page.getByRole('button',{name:/Переименовать/}).first().waitFor();
 };
 await page.goto(fixture.address);await page.getByRole('button',{name:'Сводка',exact:true}).waitFor();await page.evaluate(()=>document.fonts.ready);
 await page.getByRole('button',{name:'Сводка',exact:true}).click();await wait(500);await image(page,'gui-summary.png');
 await page.getByRole('button',{name:'Реплики',exact:true}).click();await wait(350);await image(page,'gui-transcript.png');
 await page.getByRole('button',{name:'Текст',exact:true}).click();await wait(300);await image(page,'gui-text.png');
 const reading=await mkdtemp(path.join(work,'gui-reading-'));
 for(let i=0;i<45;i++){
  if(i===0) await showTab('Сводка');
  if(i===10) await showTab('Реплики');
  if(i===22) await showTab('Текст');
  if(i===32) await showTab('Участники');
  await page.screenshot({path:path.join(reading,`${String(i).padStart(4,'0')}.png`)});await wait(100);
 }
 await gif(reading,'gui-reading.gif');
 await page.locator('.list').getByText('Архитектура фоновой обработки',{exact:true}).click();
 await page.getByRole('button',{name:'Ход',exact:true}).click();await wait(500);await image(page,'gui-journal.png');
 const folder=await mkdtemp(path.join(work,'gui-journal-'));
 for(let i=0;i<50;i++){
  if(i===12) await page.getByRole('button',{name:/\+ \d+ lines/}).first().click();
  if(i===30) await page.getByRole('button',{name:/свернуть/}).first().click();
  if(i===40) await page.getByRole('button',{name:'Реплики',exact:true}).click();
  await page.screenshot({path:path.join(folder,`${String(i).padStart(4,'0')}.png`)});await wait(100);
 }
 await gif(folder,'gui-workflow.gif');
 await page.getByRole('button',{name:/Настройки/}).first().click();await wait(500);await image(page,'gui-settings.png');
 await page.getByTitle('Тема',{exact:true}).click();
 await page.getByRole('button',{name:/📼 Записи/}).click();
 await page.locator('.list').getByText('План выпуска · LocalVox',{exact:true}).click();
 await page.getByRole('button',{name:'Сводка',exact:true}).click();await wait(300);await image(page,'gui-light.png');
 await page.close();
}
const common={cols:140,rows:36};
const scenarios=[
 {name:'tui-navigation',duration_ms:13000,steps:[{at_ms:2000,keys:'j'},{at_ms:2600,keys:'j'},{at_ms:3200,keys:'k'},{at_ms:3900,keys:'k'},{at_ms:4700,keys:'l'},{at_ms:5800,keys:'2'},{at_ms:7000,keys:'j'},{at_ms:7800,keys:'j'},{at_ms:9000,keys:'3'},{at_ms:10800,keys:'\u001b'}],
  captions:[{at:0,key:'j / k',text:'Выбирай встречу в списке'},{at:4700,key:'l / Enter',text:'Открой запись'},{at:5800,key:'2 → j / k',text:'Читай реплики и выбирай нужную'},{at:9000,key:'3',text:'Весь текст в одной вкладке'},{at:10800,key:'Esc',text:'Вернись на уровень выше'}],shots:[{at:1700,name:'tui-archive.png'},{at:5500,name:'tui-summary.png'},{at:8500,name:'tui-transcript.png'},{at:10100,name:'tui-text.png'}]},
 {name:'tui-search',duration_ms:10500,steps:[{at_ms:1900,keys:'/'},{at_ms:2600,keys:'интер'},{at_ms:4200,keys:'\u001b'},{at_ms:5400,keys:'/'},{at_ms:5900,keys:'План'},{at_ms:7500,keys:'\r'},{at_ms:8500,keys:'l'}],
  captions:[{at:0,key:'/',text:'Поиск по названию встречи'},{at:2600,key:'интер',text:'Список фильтруется по мере ввода'},{at:4200,key:'Esc',text:'Очисти поиск и верни навигацию'},{at:5900,key:'План',text:'Найди нужную запись'},{at:7500,key:'Enter → l',text:'Открой найденную встречу'}],shots:[{at:3600,name:'tui-search.png'}]},
];
async function tui(){
 const suffix=process.platform==='win32'?'.exe':'';
 const binary=process.env.LOCALVOX_PROMO_TUI??path.join(root,'target/review/incremental-default-on/localvox-tui'+suffix);
 const recorder=path.join(root,'target/debug/examples/promo-capture'+suffix);
 await stat(binary);await stat(recorder);
 for(const scenario of scenarios){
  const config=path.join(work,scenario.name+'.json'),recording=path.join(work,scenario.name+'.jsonl');
  await writeFile(config,JSON.stringify({...common,...scenario}));
  await run(recorder,[binary,fixture.address,config,recording],{timeout:scenario.duration_ms+15000});
  const [header,...events]=(await readFile(recording,'utf8')).trim().split('\n').map(JSON.parse);
  const page=await context.newPage();await page.goto(fixture.address+`/terminal?cols=${header.cols}&rows=${header.rows}`);await page.evaluate(()=>window.ready);
  const folder=await mkdtemp(path.join(work,scenario.name+'-'));
  let event=0;const shots=new Set();const fps=5;
  for(let frame=0;frame<Math.ceil((scenario.duration_ms-1000)/1000*fps);frame++){
   const time=1000+frame*1000/fps;
   while(event<events.length&&events[event].at_ms<=time){await page.evaluate(bytes=>window.writeTerminal(bytes),events[event++].bytes);}
   const caption=scenario.captions.filter(c=>c.at<=time).at(-1);
   await page.evaluate(c=>{document.getElementById('keys').textContent=c.key;document.getElementById('caption').textContent=c.text;},caption);
   await page.screenshot({path:path.join(folder,`${String(frame).padStart(4,'0')}.png`)});
   for(const shot of scenario.shots.filter(s=>time>=s.at&&!shots.has(s.name))){await image(page,shot.name);shots.add(shot.name);}
  }
  await writeFile(path.join(work,scenario.name+'-screen.txt'),await page.evaluate(()=>Array.from({length:term.rows},(_,i)=>term.buffer.active.getLine(i)?.translateToString(true)).join('\n')));
  await gif(folder,scenario.name+'.gif');await page.close();
 }
}
try{
 if(!process.argv.includes('--tui-only')) await gui();
 if(!process.argv.includes('--gui-only')) await tui();
 const entries=await Promise.all((await readdir(out)).filter(n=>/\.(png|gif)$/.test(n)).sort().map(async name=>{
  const file=path.join(out,name),data=await readFile(file);
  const item={name,bytes:data.length,sha256:createHash('sha256').update(data).digest('hex')};
  if(name.endsWith('.png')) Object.assign(item,{width:data.readUInt32BE(16),height:data.readUInt32BE(20)});
  else {
   const {stdout}=await promisify(execFile)('ffprobe',['-v','error','-count_frames','-show_entries','stream=width,height,nb_read_frames:format=duration','-of','json',file],{windowsHide:true,timeout:20000});
   const probe=JSON.parse(stdout);Object.assign(item,{width:probe.streams[0].width,height:probe.streams[0].height,frames:Number(probe.streams[0].nb_read_frames),duration_sec:Number(probe.format.duration)});
  }
  if(item.bytes>3*1024*1024) throw new Error('Promo asset exceeds 3 MiB: '+name);
  return item;
 }));
 if(entries.reduce((n,item)=>n+item.bytes,0)>8*1024*1024) throw new Error('Promo assets exceed 8 MiB');
 const manifest=JSON.stringify({captured_at:new Date().toISOString(),fixture:'fictional demonstration data; actual React UI and TUI via PTY',files:entries},null,2)+'\n';
 await writeFile(path.join(work,'manifest.json'),manifest);await writeFile(path.join(out,'manifest.json'),manifest);
 console.log(entries);
}finally{await browser.close();await fixture.close();}
