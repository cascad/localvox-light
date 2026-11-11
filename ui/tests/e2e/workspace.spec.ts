import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const fixture = JSON.parse(readFileSync(fileURLToPath(new URL("../../../tests/fixtures/workspace.json", import.meta.url)), "utf8"));
const journalFixture = JSON.parse(readFileSync(fileURLToPath(new URL("../../../tests/fixtures/journal.json", import.meta.url)), "utf8"));

test("expanding journal details preserves both scroll positions until explicit follow", async ({page}) => {
  await page.setViewportSize({width:1000,height:650});
  await workspace(page);
  let append = false;
  const extra = {...journalFixture.events.find((e: {text?:string}) => e.text?.includes("Диагностическая")),id:"reader-new",text:"Новая строка для проверки чтения"};
  await page.route("**/journal**", route => route.fulfill({json:{...journalFixture,events:append ? [...journalFixture.events,extra] : journalFixture.events}}));
  await page.addStyleTag({content:".journal-scroll {max-height:180px} .main {height:360px;padding-bottom:300px}"});
  await page.getByRole("button",{name:"Ход",exact:true}).click();
  const toggle=page.getByRole("button",{name:"+ 35 lines",exact:true});
  await expect(toggle).toBeVisible();
  await page.clock.runFor(50);
  await toggle.scrollIntoViewIfNeeded();
  const positions=() => page.evaluate(() => [document.querySelector(".main")!.scrollTop,document.querySelector(".journal-scroll")!.scrollTop,window.scrollY]);
  await page.locator(".main").evaluate(el => {el.scrollTop=60;});
  const locked=await positions();
  expect(locked[0]).toBeGreaterThan(0);
  await toggle.evaluate(node => node.click());
  await page.clock.runFor(50);
  expect(await positions()).toEqual(locked);
  append=true;
  await page.clock.runFor(2600);
  await expect(page.getByRole("button",{name:/новые: 1/})).toBeVisible();
  expect(await positions()).toEqual(locked);
  await page.getByRole("button",{name:"− свернуть",exact:true}).evaluate(node => node.click());
  await page.clock.runFor(50);
  expect(await positions()).toEqual(locked);
  await page.getByRole("button",{name:/новые: 1/}).click();
  await page.clock.runFor(50);
  const atTail=await page.locator(".journal-scroll").evaluate(el => el.scrollHeight-el.clientHeight-el.scrollTop);
  expect(atTail).toBeLessThan(2);
});

test("settings show editable presets and stage reusable LLM servers", async ({page}) => {
  await workspace(page);
  const settings = [
    {key:"LOCALVOX_LLM_MODEL",group:"LLM",label:"Модель",kind:"text",value:null,default_value:"qwen3.5:9b",applies:"restart",hint:"Модель обработки",set:false},
    {key:"LOCALVOX_LLM_BASE_URL",group:"LLM",label:"Адрес сервера",kind:"text",value:null,default_value:"http://localhost:11434/v1",applies:"restart",hint:"HTTP(S) endpoint модели",set:false},
    {key:"LOCALVOX_LLM_SERVER_URLS",group:"LLM",label:"Список",kind:"servers",value:null,default_value:'["http://localhost:11434/v1"]',applies:"live",hint:"",set:false},
  ];
  const sent: Record<string,string|null>[]=[];
  await page.route("**/api/settings", route => {
    if(route.request().method()==="POST") { const {set}=route.request().postDataJSON();sent.push(set);for(const s of settings)if(s.key in set)s.value=set[s.key];return route.fulfill({json:{msg:"Сохранено",need_restart:["Адрес сервера"]}}); }
    return route.fulfill({json:{path:"test/.env",defaults_path:"test/.env.defaults",exists:true,settings,extra:[]}});
  });
  await page.getByRole("button",{name:/⚙ Настройки/}).click();
  await expect(page.locator('.set-row input').first()).toHaveValue("qwen3.5:9b");
  await expect(page.locator(".pane")).not.toContainText("зашито в коде");
  await expect(page.locator(".set-source").first()).toHaveText("Предустановка");
  await page.getByRole("button",{name:"Добавить сервер",exact:true}).click();
  const url=page.getByRole("textbox",{name:"Адрес нового LLM-сервера"});
  await url.fill("https://user:secret@models.example/v1");
  await page.getByRole("button",{name:"Добавить в список"}).click();
  await expect(page.locator(".llm-server-add .err")).toBeVisible();
  expect(sent).toEqual([]);
  await url.fill("https://models.example/v1/");
  await page.getByRole("button",{name:"Добавить в список"}).click();
  await expect(page.getByRole("combobox",{name:"Адрес сервера"})).toHaveValue("https://models.example/v1");
  expect(sent).toEqual([]);
  await expect(page.locator(".pane")).toHaveScreenshot("settings-presets-and-servers.png");
  await page.getByRole("button",{name:/^Сохранить/}).click();
  await expect.poll(()=>sent.length).toBe(1);
  expect(JSON.parse(sent[0].LOCALVOX_LLM_SERVER_URLS!)).toEqual(["http://localhost:11434/v1","https://models.example/v1"]);
  await page.getByRole("button",{name:/Записи/}).click();
  await page.getByRole("button",{name:/⚙ Настройки/}).click();
  await expect(page.getByRole("combobox",{name:"Адрес сервера"})).toHaveValue("https://models.example/v1");
});

test("missing summary is a neutral document instead of a service error", async ({ page }) => {
  await workspace(page);
  await page.route("**/summary", route => route.fulfill({ json: { available: false, blocks: [], provenance: null } }));
  await page.clock.runFor(3100);
  await expect(page.locator(".doc")).toContainText("Сводка ещё не создана");
  await expect(page.locator(".doc .err")).toHaveCount(0);
  await expect(page.locator(".doc")).not.toContainText("localvox-process");
  await page.getByRole("button", {name:"Текст",exact:true}).click();
  await expect(page.locator(".book")).toContainText(fixture.readable.lines[0].text);
});

test("queue cancellation waits for acknowledgement and resumes without a full recook", async ({ page }) => {
  await workspace(page);
  let state = "running"; const posts: string[] = [];
  let release!: () => void; const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/queue", route => route.fulfill({ json: { queue: [{ ...fixture.queue[0], phase: "summary", state, stuck: false, last_error: null }] } }));
  await page.route("**/api/jobs", route => route.fulfill({ json: { running: state === "running" ? 1 : 0, pending: state === "pending" ? 1 : 0, failed: 0, cancelling: state === "cancelling" ? 1 : 0, cancelled: state === "cancelled" ? 1 : 0 } }));
  await page.route(/\/api\/sessions(\?.*)?$/, route => route.fulfill({ json: fixture.sessions.map((session: object) => ({ ...session, job: state })) }));
  await page.route("**/cancel", async route => {
    posts.push(new URL(route.request().url()).pathname); state = "cancelling";
    await gate; state = "cancelled";
    await route.fulfill({ json: { ok: true, msg: "Обработка отменена; текст сохранён" } });
  });
  await page.route(/\/(retry|recook)(\?.*)?$/, route => {
    posts.push(new URL(route.request().url()).pathname); state = "pending";
    return route.fulfill({ json: { ok: true, msg: "Обработка продолжится" } });
  });
  await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
  await page.getByRole("button", { name: "Отменить обработку", exact: true }).click();
  const pending = page.getByRole("button", { name: "Остановка…", exact: true });
  await expect(pending).toBeDisabled(); await pending.evaluate((node: HTMLButtonElement) => node.click());
  try {
    await page.clock.runFor(3100);
    await expect(page.locator(".qrow")).toHaveClass(/cancelling/);
    await expect(page.locator(".bar > .queue")).toContainText("Останавливается");
    await expect(page.getByRole("button", { name: "Продолжить обработку", exact: true })).toHaveCount(0);
    expect(posts).toEqual(["/api/sessions/20261031_demo/cancel"]);
  } finally { release(); }
  await expect(page.locator(".qrow")).toHaveClass(/cancelled/);
  await expect(page.getByText("Продолжение — только вручную", { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 480, height: 850 }); await page.mouse.move(0, 0);
  await expect(page.locator(".pane")).toHaveScreenshot("queue-cancelled-dark-480.png");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
  await page.getByRole("button", { name: "Продолжить обработку", exact: true }).click();
  await expect(page.locator(".qrow")).toHaveClass(/pending/);
  expect(posts).toEqual(["/api/sessions/20261031_demo/cancel", "/api/sessions/20261031_demo/retry"]);
});

for (const status of [409, 503]) {
  test(`queue cancellation ${status} keeps the source and permits an explicit retry`, async ({ page }) => {
    await workspace(page); const posts: string[] = [];
    await page.route("**/cancel", route => {
      posts.push(new URL(route.request().url()).pathname);
      return route.fulfill({ status, json: { error: "stop not accepted; files retained" } });
    });
    await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
    const cancel = page.getByRole("button", { name: "Отменить обработку", exact: true });
    await cancel.click(); await expect(page.getByText("stop not accepted; files retained", { exact: true })).toBeVisible();
    await expect(cancel).toBeEnabled(); await expect(page.locator(".qrow")).toHaveClass(/running/);
    expect(posts).toEqual(["/api/sessions/20261031_demo/cancel"]);
  });
}

test("queue cancellation timeout preserves its stopping state instead of declaring completion", async ({ page }) => {
  await workspace(page); let stopping = false;
  await page.route("**/api/queue", route => route.fulfill({ json: { queue: [{ ...fixture.queue[0], state: stopping ? "cancelling" : "running" }] } }));
  await page.route("**/cancel", route => { stopping = true; return route.fulfill({ status: 409, json: { error: "stop acknowledgement is pending" } }); });
  await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
  await page.getByRole("button", { name: "Отменить обработку", exact: true }).click();
  await expect(page.getByText("stop acknowledgement is pending", { exact: true })).toBeVisible();
  await expect(page.locator(".qrow")).toHaveClass(/cancelling/);
  await expect(page.getByRole("button", { name: "Продолжить обработку", exact: true })).toHaveCount(0);
  await page.clock.runFor(5100); await expect(page.locator(".qrow")).toHaveClass(/cancelling/);
});

test("document cancellation preserves readable text and shows the unfinished summary as cancelled", async ({ page }) => {
  await workspace(page); let state = "running"; const posts: string[] = [];
  await page.route(/\/api\/sessions(\?.*)?$/, route => route.fulfill({ json: fixture.sessions.map((session: object) => ({ ...session, job: state })) }));
  await page.route("**/progress", route => route.fulfill({ json: {
    ...fixture.progress, execution: { state, message: state === "cancelled" ? "Обработка отменена; продолжение вручную" : "Обрабатывается", next: [] },
    stages: fixture.progress.stages.map((stage: { stage: string }) => stage.stage === "summary" ? { ...stage, state: state === "cancelled" ? "cancelled" : "running", ended_at: state === "cancelled" ? "2026-10-31T07:00:25Z" : null } : stage),
  } }));
  await page.route(/\/(cancel|retry|recook)(\?.*)?$/, route => {
    const path = new URL(route.request().url()).pathname; posts.push(path);
    state = path.endsWith("/cancel") ? "cancelled" : "pending";
    return route.fulfill({ json: { msg: "Сохранено" } });
  });
  await page.route("**/processed", route => route.fulfill({ json: { lines: fixture.transcript.lines, version_id: 1, from_transcript: true } }));
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  const text = fixture.transcript.lines.map((line: { text: string }) => line.text).join(" ");
  await expect(page.locator("[data-plain-text]")).toHaveText(text);
  await page.getByRole("button", { name: "Отменить обработку", exact: true }).click();
  await expect(page.getByRole("button", { name: "Продолжить обработку", exact: true })).toBeVisible();
  await expect(page.locator("[data-plain-text]")).toHaveText(text);
  await page.getByRole("button", { name: "Ход", exact: true }).click();
  await expect(page.locator(".step.cancelled")).toContainText("Сводка");
  await expect(page.locator(".step.cancelled .st")).toHaveClass(/mute/);
  await expect(page.locator(".step.done").filter({ hasText: "Распознавание" })).toBeVisible();
  await page.getByRole("button", { name: "Продолжить обработку", exact: true }).click();
  await expect(page.getByRole("button", { name: "Продолжить обработку", exact: true })).toHaveCount(0);
  expect(posts).toEqual(["/api/sessions/20261031_demo/cancel", "/api/sessions/20261031_demo/retry"]);
});

async function workspace(page: Page, options: { journalError?: boolean; record?: boolean; jobsError?: boolean; partial?: boolean } = {}) {
  await page.clock.install({ time: new Date("2026-10-31T07:00:25Z") });
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url()); const path = url.pathname;
    let body: unknown = {};
    if (path === "/api/sessions") body = options.partial ? fixture.sessions.map((s: object,i:number)=>i===0 ? {...s,recording:true,cooked:false,job:"waiting_input"} : s) : fixture.sessions;
    else if (path === "/api/record") body = { ...fixture.record, recording: !!options.record, session: options.record ? "20261031_demo" : null };
    else if (path === "/api/jobs") {
      if (options.jobsError) return route.fulfill({ status: 503, json: { error: "synthetic queue unavailable" } });
      body = fixture.jobs;
    }
    else if (path === "/api/queue") body = { queue: fixture.queue };
    else if (path.endsWith("/progress")) body = fixture.progress;
    else if (path.endsWith("/journal")) {
      if (options.journalError) return route.fulfill({ status: 503, json: { error: "synthetic journal failure" } });
      body = journalFixture;
    } else if (path.endsWith("/summary")) body = fixture.summary;
    else if (path.endsWith("/transcript")) body = options.partial ? {lines:growingLines(80),partial:growingInfo(1)} : fixture.transcript;
    else if (path.endsWith("/processed")) body = options.partial ? {...fixture.readable,from_transcript:true,lines:growingLines(80),partial:growingInfo(1)} : fixture.readable;
    else if (path.endsWith("/versions")) body = { versions: fixture.versions };
    else if (path.endsWith("/speakers")) body = { speakers: fixture.speakers };
    else if (path.endsWith("/sources")) body = { sources: fixture.sources };
    else if(path.endsWith("/playback")) body = options.partial ? {duration_sec:100,snapshot:growingInfo(1).snapshot,partial:growingInfo(1)} : {duration_sec:120,snapshot:null,partial:null};
    else if (path.endsWith("/peaks")) body = { duration_sec: 120, peaks: Array.from({ length: 320 }, (_, i) => (i % 9 + 1) / 12) };
    else if (path === "/api/slots/notes") body = { slots: fixture.slots };
    else if (path === "/api/slots") body = { slots: ["идеи"] };
    else if (path === "/api/asks") body = { asks: fixture.asks };
    else if (path === "/api/asks/demo-ask") body = fixture.ask;
    else if (path === "/api/settings") body = fixture.settings;
    else if (path === "/api/langs") body = { langs: ["ru", "en"] };
    else if (path.endsWith("audio.wav")) return route.fulfill({ status: 404, body: "synthetic fixture has no sound" });
    else if (route.request().method() !== "GET") body = { msg: "Сохранено", session: "20261031_demo", need_restart: [] };
    await route.fulfill({ json: body });
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: /Обсуждение архитектуры/ })).toBeVisible();
}

async function watchReadAborts(page: Page, resources = ["summary", "processed", "transcript", "versions", "speakers"]) {
  const aborted: string[] = [];
  await page.exposeFunction("reportReadAbort", (path: string) => { aborted.push(path); });
  await page.evaluate((resources) => {
    const original = window.fetch.bind(window);
    const report = (window as unknown as { reportReadAbort: (path: string) => Promise<void> }).reportReadAbort;
    window.fetch = (input, init) => {
      const path = new URL(input instanceof Request ? input.url : String(input), location.href).pathname;
      if (!resources.includes(path.split("/").at(-1) ?? "")) return original(input, init);
      const onAbort = () => { void report(path); };
      init?.signal?.addEventListener("abort", onAbort, { once: true });
      return original(input, init).finally(() => init?.signal?.removeEventListener("abort", onAbort));
    };
  }, resources);
  return aborted;
}

test("completed stage tail uses its own phase clock and preserves a fast child operation", async ({page}) => {
  await workspace(page);
  let run="r1";
  const end="2026-10-31T06:56:19Z";
  await page.route("**/progress",route=>route.fulfill({json:{running:false,
    stages:[{stage:"summary",state:"done",started_at:"2026-10-31T06:50:00Z",ended_at:end}],
    execution:{run,state:"done",current:null,operation:null,next:[],message:"Готово"}
  }}));
  await page.route("**/journal**",route=>route.fulfill({json:{...journalFixture,reset:true,events:[
    {id:"child",run:"r1",stage:"summary",operation:"check",kind:"operation_end",at:"2026-10-31T06:53:31Z",label:"Часть 8/23: проверка",elapsed_sec:0.9353011},
    {id:"stage",run:"r1",stage:"summary",kind:"stage",status:"done",at:end,label:"summary"}
  ]}}));
  await page.getByRole("button",{name:"Ход",exact:true}).click();
  const phase=page.locator(".journal-row").filter({hasText:"summary"});
  const child=page.locator(".journal-row").filter({hasText:"Часть 8/23"});
  await expect(phase.getByLabel("Длительность операции")).toHaveText("6:19");
  await expect(child.getByLabel("Длительность операции")).toHaveText("<1 с");
  await page.clock.runFor(3100);
  await expect(phase.getByLabel("Длительность операции")).toHaveText("6:19");
  run="another-attempt";
  await page.clock.runFor(3100);
  await expect(phase.getByLabel("Длительность операции")).toHaveText("время неизвестно");
  await expect(child.getByLabel("Длительность операции")).toHaveText("<1 с");
});

test("voice grouping stays active after all audio was read and retains its measured time", async ({ page }) => {
  await workspace(page);
  let ended = false;
  const start = {id:"group-start",run:"r1",operation:"group",parent:"worker",stage:"transcribe",kind:"operation_start",at:"2026-10-31T07:00:20Z",label:"Группировка голосов · дорожка 1"};
  await page.route("**/progress", route => route.fulfill({json:{
    running:true,stages:[{stage:"transcribe",state:"running",started_at:"2026-10-31T07:00:00Z",done:18240,total:18240}],
    execution:{run:"r1",operation:"worker",state:"running",phase:"transcribe",current:"transcribe",since:"2026-10-31T07:00:00Z",next:["summary"],message:"Этап выполняется"}
  }}));
  await page.route("**/journal**", route => route.fulfill({json:{...journalFixture,
    events:ended ? [start,{...start,id:"group-end",kind:"operation_end",at:"2026-10-31T07:00:23Z",elapsed_sec:3.5}] : [start],
    next_cursor:ended ? "group-ended" : "group-running",previous_cursor:"",has_more:false,reset:true
  }}));
  await page.getByRole("button",{name:"Ход",exact:true}).click();
  const row=page.locator(".journal-row").filter({hasText:"Группировка голосов"});
  await expect(row.locator(".journal-state").first()).toHaveText("идёт");
  await expect(row).toContainText("0:05");
  await expect(page.locator(".step.running")).toHaveCount(1);
  await page.clock.runFor(2100);
  await expect(row).toContainText("0:07");
  ended=true;
  await page.clock.runFor(3100);
  await expect(row.locator(".journal-state").first()).toHaveText("операция завершилась");
  await expect(row).toContainText("0:03");
});

test("journal folds exactly 35 lines and command arguments have separate styles", async ({ page }) => {
  await workspace(page);
  await page.getByRole("button", { name: "Ход", exact: true }).click();
  const toggle = page.getByRole("button", { name: "+ 35 lines", exact: true });
  await expect(toggle).toBeVisible();
  await expect(page.getByText("Диагностическая строка 1", { exact: true })).not.toBeVisible();
  await toggle.click();
  await expect(page.getByText("Диагностическая строка 1", { exact: true })).toBeVisible();
  await expect(page.locator(".journal-command")).toHaveCSS("color", "rgb(133, 180, 220)");
  await expect(page.locator(".journal-arg").first()).toHaveCSS("color", "rgb(194, 165, 216)");
  await expect(page.locator(".journal")).toHaveScreenshot("journal-expanded-dark.png");
});

test("early transcription defaults to on and can be saved off on or reset without restart", async ({page}) => {
  await workspace(page);
  const key = "LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE";
  let value: string | null = null;
  const sent: Record<string,string|null>[] = [];
  await page.route("**/api/settings", route => {
    if (route.request().method() === "POST") {
      const {set} = route.request().postDataJSON();
      sent.push(set); value = set[key];
      return route.fulfill({json:{msg:"Сохранено и применено",need_restart:[]}});
    }
    return route.fulfill({json:{path:"synthetic/.env",defaults_path:"synthetic/.env.defaults",exists:true,extra:[],settings:[{
      key,group:"Автоварка",label:"Распознавать во время записи",kind:"bool",value,default_value:"on",applies:"live",set:value !== null,
      hint:"Готовые фрагменты обрабатываются заранее. Выключение сохраняет текущую порцию; после Stop — обычная обработка",
    }]}});
  });
  await page.getByRole("button",{name:/⚙ Настройки/}).click();
  const toggle = page.getByRole("combobox",{name:"Распознавать во время записи",exact:true});
  const save = async () => {
    await page.getByRole("button",{name:"Сохранить (1)",exact:true}).click();
    await expect(page.getByRole("button",{name:"Сохранено",exact:true})).toBeVisible();
  };
  await expect(toggle).toBeVisible();
  await expect(toggle).toHaveValue("on");
  await expect(page.locator(".set-source")).toHaveText("Предустановка");
  await toggle.selectOption("off");
  await save();
  await expect(toggle).toHaveValue("off");
  await expect(page.getByText("Перезапускать ничего не нужно.",{exact:false})).toBeVisible();
  await expect(page.locator(".set-source")).toHaveText("Из .env");
  await toggle.selectOption("on");
  await page.getByRole("button",{name:"Отменить правки",exact:true}).click();
  await expect(toggle).toHaveValue("off");
  expect(sent).toEqual([{[key]:"off"}]);
  await toggle.selectOption("on");
  await save();
  await expect(toggle).toHaveValue("on");
  await toggle.selectOption("");
  await save();
  await expect(toggle).toHaveValue("on");
  await expect(page.locator(".set-source")).toHaveText("Предустановка");
  expect(sent).toEqual([{[key]:"off"},{[key]:"on"},{[key]:null}]);
});

test("only summary is optional and the Text tab reads the primary transcript without cleanup", async ({ page }) => {
  await workspace(page);
  const setting = { key: "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY", group: "После распознавания", label: "Сводка · summary", hint: "включено", kind: "bool", value: "on", applies: "live", set: true };
  const sent: unknown[] = [];
  await page.route("**/api/settings", async route => {
    if (route.request().method() === "GET") return route.fulfill({ json: { path: "synthetic/.env", exists: true, settings: [setting], extra: [] } });
    const { set } = route.request().postDataJSON(); sent.push(set); setting.value = set[setting.key];
    return route.fulfill({ json: { msg: "Сохранено и применено", need_restart: [] } });
  });
  await page.getByRole("button", { name: /⚙ Настройки/ }).click();
  await expect(page.getByText("Транскрибация обязательна.", { exact: false })).toBeVisible();
  const summary = page.getByRole("combobox", { name: "Сводка · summary", exact: true });
  await summary.selectOption("off");
  await page.getByRole("button", { name: "Отменить правки", exact: true }).click();
  expect(sent).toEqual([]);
  await expect(summary).toHaveValue("on");
  await summary.selectOption("off");
  await page.getByRole("button", { name: "Сохранить (1)", exact: true }).click();
  expect(sent).toEqual([{ LOCALVOX_LIGHT_AUTOCOOK_SUMMARY: "off" }]);
  await page.route("**/processed", route => route.fulfill({ json: { lines: fixture.transcript.lines, version_id: 1, from_transcript: true, provenance: null, omitted: 0, rejected: 0 } }));
  await page.getByRole("button", { name: /📼 Записи/ }).click();
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  await expect(page.locator(".book")).toContainText(fixture.transcript.lines[0].text);
  await expect(page.getByText("Отдельной очищенной версии нет.", { exact: false })).toBeVisible();
  await expect(page.getByText(/поправлено.*строк/)).toHaveCount(0);
  await page.getByRole("button", { name: /⚙ Настройки/ }).click();
  await summary.selectOption("on");
  await page.getByRole("button", { name: "Сохранить (1)", exact: true }).click();
  expect(sent[1]).toEqual({ LOCALVOX_LIGHT_AUTOCOOK_SUMMARY: "on" });
});

test("raw Text is a single body and the copy shortcut includes offscreen text without intercepting editors", async ({ page }) => {
  await workspace(page);
  let lines = [
    { who: "Анна", start_sec: 0, end_sec: 2, text: "Первая реплика." },
    { who: "Борис", start_sec: 3600, end_sec: 3602, text: "Вторая реплика." },
  ];
  const copied: string[] = [];
  await page.exposeFunction("capturePlainText", (text: string) => { copied.push(text); });
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: { writeText: (text: string) => (window as unknown as { capturePlainText: (text: string) => Promise<void> }).capturePlainText(text) } }));
  await page.route("**/processed", route => route.fulfill({ json: { lines, version_id: 1, from_transcript: true, provenance: null, omitted: 0, rejected: 0 } }));
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  await expect(page.locator(".book p")).toHaveCount(1);
  await expect(page.locator("[data-plain-text]")).toHaveText("Первая реплика. Вторая реплика.");
  await expect(page.locator(".book")).not.toContainText(/Анна|Борис|00:00|—/);
  await page.keyboard.press("Control+Shift+C");
  await expect.poll(() => copied).toEqual(["Первая реплика. Вторая реплика."]);
  await expect(page.locator(".doc")).toHaveScreenshot("raw-text.png");
  lines = Array.from({ length: 500 }, (_, i) => ({ who: "Голос", start_sec: i, end_sec: i + 1, text: `Фрагмент ${i}.` }));
  const full = lines.map(line => line.text).join(" ");
  await page.clock.runFor(3100);
  await expect(page.locator("[data-plain-text]")).toHaveText(full);
  expect(await page.locator("[data-plain-text]").evaluate(element => {
    const text = element.lastElementChild!.lastChild!;
    const range = document.createRange(); range.setStart(text, text.textContent!.length - 12); range.setEnd(text, text.textContent!.length);
    return range.getBoundingClientRect().top > window.innerHeight;
  })).toBe(true);
  await page.keyboard.press("Control+Shift+C");
  await expect.poll(() => copied.length).toBe(2);
  expect(copied[1]).toBe(full);
  const search = page.getByPlaceholder("Поиск по архиву…");
  await search.fill("черновик");
  await search.press("Control+Shift+C");
  expect(copied.length).toBe(2);
  await expect(search).toHaveValue("черновик");
  await page.locator(".doc-head").getByRole("button", { name: /Копировать/ }).click();
  await expect.poll(() => copied.length).toBe(3);
  expect(copied[2]).toBe(full);
});

test("copy shortcut uses the physical key across layouts and respects IME and editors", async ({ page }) => {
  await workspace(page);
  const copied: string[] = [];
  await page.exposeFunction("captureKeyCopy", (text: string) => { copied.push(text); });
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: { writeText: (text: string) => (window as unknown as { captureKeyCopy: (text: string) => Promise<void> }).captureKeyCopy(text) } }));
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  await expect(page.locator(".book")).not.toBeEmpty();
  for (const [index, key] of ["с", "ψ", "ч", "中"].entries()) {
    const handled = await page.evaluate(key => !window.dispatchEvent(new KeyboardEvent("keydown", { key, code: "KeyC", ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true })), key);
    expect(handled).toBe(true);
    await expect.poll(() => copied.length).toBe(index + 1);
  }
  for (const extra of [{ code: "KeyV" }, { isComposing: true }, { repeat: true }, { altKey: true }]) {
    const handled = await page.evaluate(extra => !window.dispatchEvent(new KeyboardEvent("keydown", { key: "c", code: "KeyC", ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true, ...extra })), extra);
    expect(handled).toBe(false);
  }
  await page.getByPlaceholder("Поиск по архиву…").evaluate(element => element.dispatchEvent(new KeyboardEvent("keydown", { key: "с", code: "KeyC", ctrlKey: true, shiftKey: true, bubbles: true })));
  expect(copied.length).toBe(4);
  await page.evaluate(() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "ц", code: "KeyC", metaKey: true, shiftKey: true, bubbles: true })));
  await expect.poll(() => copied.length).toBe(5);
  expect(new Set(copied).size).toBe(1);
});

for (const endpoint of ["sessions", "jobs", "record"]) {
  test(`workspace ${endpoint} polling never overlaps a slow background GET`, async ({ page }) => {
    await workspace(page);
    let calls = 0; let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    await page.route(endpoint === "sessions" ? "**/api/sessions?**" : `**/api/${endpoint}`, async route => {
      calls++;
      await gate;
      await route.fulfill({ json: endpoint === "sessions" ? fixture.sessions : endpoint === "jobs" ? fixture.jobs : fixture.record }).catch(() => {});
    });
    await page.clock.runFor(5100);
    await expect.poll(() => calls).toBeGreaterThan(0);
    await page.clock.runFor(5100);
    expect(calls).toBe(1);
    release();
  });
}

test("a slow archive read does not delay jobs or recording status", async ({ page }) => {
  await workspace(page);
  let pending = false; let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/sessions?**", async route => { pending = true; await gate; await route.fulfill({ json: fixture.sessions }); });
  await page.route("**/api/jobs", route => route.fulfill({ json: { running: 0, pending: 6, failed: 0 } }));
  await page.route("**/api/record", route => route.fulfill({ json: { ...fixture.record, recording: true, session: "20261031_demo" } }));
  await page.clock.runFor(5100);
  await expect.poll(() => pending).toBe(true);
  await expect(page.locator(".nav").getByRole("button", { name: /Очередь/ })).toContainText("6");
  await expect(page.getByRole("button", { name: "Остановить", exact: true })).toBeVisible();
  release();
});

for (const endpoint of ["jobs", "record"]) {
  test(`a slow ${endpoint} read does not delay the archive`, async ({ page }) => {
    await workspace(page);
    let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    await page.route(`**/api/${endpoint}`, async route => { await gate; await route.fulfill({ json: endpoint === "jobs" ? fixture.jobs : fixture.record }); });
    await page.route("**/api/sessions?**", route => route.fulfill({ json: fixture.sessions.map((session: object) => ({ ...session, title: "Обновлённый архив" })) }));
    await page.clock.runFor(5100);
    await expect(page.getByRole("heading", { name: /^Обновлённый архив ·/ })).toBeVisible();
    release();
  });
}

for (const want of [true, false]) {
  test(`confirmed ${want ? "start" : "stop"} cannot be rolled back by an older background status`, async ({ page }) => {
    await workspace(page, { record: !want });
    const aborted = await watchReadAborts(page, ["record"]);
    let active = !want; let reads = 0; let served = false; let release!: () => void;
    const controls: string[] = [];
    const gate = new Promise<void>(resolve => { release = resolve; });
    await page.route("**/api/record**", async route => {
      const path = new URL(route.request().url()).pathname;
      if (route.request().method() === "POST") {
        controls.push(path); active = path.endsWith("/start");
        return route.fulfill({ json: { msg: "Принято", session: "20261031_demo" } });
      }
      if (++reads === 1) {
        await gate;
        await route.fulfill({ json: { ...fixture.record, recording: !want, session: want ? null : "20261031_demo" } }).catch(() => {});
        served = true;
        return;
      }
      return route.fulfill({ json: { ...fixture.record, recording: active, session: active ? "20261031_demo" : null } });
    });
    await page.clock.runFor(1100);
    await expect.poll(() => reads).toBeGreaterThan(0);
    await page.clock.pauseAt(new Date(await page.evaluate(() => Date.now() + 1000)));
    await page.getByRole("button", { name: want ? "Запись" : "Остановить", exact: true }).click();
    await expect(page.getByRole("button", { name: want ? "Остановить" : "Запись", exact: true })).toBeVisible();
    await expect.poll(() => aborted.includes("/api/record")).toBe(true);
    if (want) await expect(page.locator(".bar .recpill:not(.waiting)")).toBeVisible();
    release();
    await expect.poll(() => served).toBe(true);
    await page.clock.runFor(50);
    await expect(page.getByRole("button", { name: want ? "Остановить" : "Запись", exact: true })).toBeVisible();
    expect(aborted).toContain("/api/record");
    expect(controls).toEqual([`/api/record/${want ? "start" : "stop"}`]);
  });
}

test("accepted import refresh cancels the old archive without losing its new selection", async ({ page }) => {
  await workspace(page);
  const aborted = await watchReadAborts(page, ["sessions"]);
  let imported = false; let pending = false; let served = false; let posts = 0; let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/sessions?**", async route => {
    if (!imported) {
      pending = true; await gate;
      await route.fulfill({ json: fixture.sessions }).catch(() => {});
      served = true; return;
    }
    return route.fulfill({ json: [...fixture.sessions, { ...fixture.sessions[0], name: "accepted-fresh", title: "Новая принятая запись" }] });
  });
  await page.route("**/api/ingest", route => { posts++; imported = true; return route.fulfill({ json: { session: "accepted-fresh" } }); });
  await page.clock.runFor(5100);
  await expect.poll(() => pending).toBe(true);
  await page.getByPlaceholder("Поиск по архиву…").fill("https://example.org/fresh");
  await page.getByRole("button", { name: "↓ Расшифровать по ссылке", exact: true }).click();
  await expect(page.getByRole("heading", { name: /^Новая принятая запись ·/ })).toBeVisible();
  await expect.poll(() => aborted.includes("/api/sessions")).toBe(true);
  release();
  await expect.poll(() => served).toBe(true);
  await page.clock.runFor(50);
  await expect(page.getByRole("heading", { name: /^Новая принятая запись ·/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Ход", exact: true })).toHaveAttribute("aria-selected", "true");
  expect(posts).toBe(1);
});

test("archive status polling preserves rows during an error and recovers after backoff", async ({ page }) => {
  await workspace(page);
  let failed = true;
  await page.route("**/api/sessions?**", route => failed
    ? route.fulfill({ status: 503, json: { error: "workspace archive offline" } })
    : route.fulfill({ json: fixture.sessions.map((session: object) => ({ ...session, title: "Архив снова доступен" })) }));
  await page.clock.runFor(5100);
  await expect(page.getByText("workspace archive offline", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: /Обсуждение архитектуры/ })).toBeVisible();
  failed = false;
  await page.clock.runFor(5100);
  await expect(page.getByText("workspace archive offline", { exact: true })).toBeVisible();
  await page.clock.runFor(5100);
  await expect(page.getByRole("heading", { name: /^Архив снова доступен ·/ })).toBeVisible();
  await expect(page.getByText("workspace archive offline", { exact: true })).toHaveCount(0);
});

test("slow document refresh is serialized and coalesced without hiding the first usable answer", async ({ page }) => {
  await workspace(page);
  let calls = 0;
  let first!: () => void; let next!: () => void;
  const firstGate = new Promise<void>(resolve => { first = resolve; });
  const nextGate = new Promise<void>(resolve => { next = resolve; });
  await page.route("**/processed", async route => {
    const call = ++calls;
    if (call === 1) await firstGate;
    if (call === 2) await nextGate;
    await route.fulfill({ json: { ...fixture.readable, from_transcript: true, lines: [{ who: "Голос", text: call === 1 ? "Текущий медленный ответ" : "Свежий ответ", start_sec: 0, end_sec: 1 }] } });
  });
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  await expect.poll(() => calls).toBeGreaterThan(0);
  await page.clock.runFor(7100);
  expect(calls).toBe(1);
  first();
  await expect(page.getByText("Текущий медленный ответ", { exact: true })).toBeVisible();
  await expect.poll(() => calls).toBe(2);
  await expect(page.getByText("загрузка…", { exact: true })).toHaveCount(0);
  next();
  await expect(page.getByText("Свежий ответ", { exact: true })).toBeVisible();
  await page.clock.runFor(1000);
  expect(calls).toBe(2);
});

test("the final document refresh survives processing finishing during a slow read", async ({ page }) => {
  await workspace(page);
  let calls = 0; let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/processed", async route => {
    const call = ++calls;
    if (call === 1) await gate;
    await route.fulfill({ json: { ...fixture.readable, from_transcript: true, lines: [{ text: call === 1 ? "Первый снимок" : "Итоговый документ", who: "Голос", start_sec: 0, end_sec: 1 }] } });
  });
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  await expect.poll(() => calls).toBe(1);
  await page.route("**/api/sessions?**", route => route.fulfill({ json: fixture.sessions.map((session: object) => ({ ...session, job: null })) }));
  await page.clock.runFor(5100);
  await expect(page.locator(".doc-head .st")).toHaveText("расшифрована");
  expect(calls).toBe(1);
  release();
  await expect(page.getByText("Итоговый документ", { exact: true })).toBeVisible();
  await page.clock.runFor(6100);
  expect(calls).toBe(2);
});

test("document errors recover on the next refresh without switching tabs", async ({ page }) => {
  await workspace(page);
  let failed = true;
  await page.route("**/processed", route => failed
    ? route.fulfill({ status: 503, json: { error: "artifact temporarily offline" } })
    : route.fulfill({ json: fixture.readable }));
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  await expect(page.getByText("artifact temporarily offline", { exact: true })).toBeVisible();
  failed = false;
  await page.clock.runFor(3100);
  await expect(page.getByText("Отделим интерфейс от движка.", { exact: true })).toBeVisible();
  await expect(page.getByText("artifact temporarily offline", { exact: true })).toHaveCount(0);
});

for (const [tab, endpoints] of [
  ["Сводка", ["summary"]], ["Текст", ["processed"]],
  ["Реплики", ["transcript", "versions"]], ["Участники", ["speakers"]],
] as const) {
  test(`leaving ${tab} cancels every pending artifact GET`, async ({ page }) => {
    await workspace(page);
    if (tab === "Сводка") await page.getByRole("button", { name: "Текст", exact: true }).click();
    const aborted = await watchReadAborts(page);
    const requested = new Set<string>();
    let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    await page.route(/\/api\/sessions\/[^/]+\/(summary|processed|transcript|versions|speakers)$/, async route => {
      const endpoint = new URL(route.request().url()).pathname.split("/").at(-1)!;
      if (!(endpoints as readonly string[]).includes(endpoint)) return route.fallback();
      requested.add(endpoint);
      await gate;
      await route.fulfill({ json: {
        ...fixture.readable, from_transcript: true,
        lines: [{ who: "Голос", text: "Поздний закрытый документ", start_sec: 0, end_sec: 1 }],
        blocks: [{ kind: "paragraph", text: "Поздний закрытый документ", sources: [] }],
        versions: fixture.versions,
        speakers: [{ ...fixture.speakers[0], name: "Поздний закрытый документ" }],
      } }).catch(() => {}); // the browser has already cancelled this request
    });
    await page.getByRole("button", { name: tab, exact: true }).click();
    await expect.poll(() => requested.size).toBe(endpoints.length);
    await page.getByRole("button", { name: tab === "Сводка" ? "Текст" : "Сводка", exact: true }).click();
    await expect.poll(() => endpoints.every(endpoint => aborted.some(path => path.endsWith(`/${endpoint}`)))).toBe(true);
    release();
    await expect(page.getByText("Поздний закрытый документ", { exact: true })).toHaveCount(0);
  });
}

test("changing the transcript version cancels its pending refresh and loads the selected version", async ({ page }) => {
  await workspace(page);
  const aborted = await watchReadAborts(page);
  let calls = 0; let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/versions", route => route.fulfill({ json: { versions: [...fixture.versions, { ...fixture.versions[0], id: 7, best: false }] } }));
  await page.route("**/transcript", async route => {
    const call = ++calls;
    if (call === 2) await gate;
    await route.fulfill({ json: { lines: [{ ...fixture.transcript.lines[0], text: call === 1 ? "Исходная версия" : call === 2 ? "Устаревшее обновление версии" : "Выбранная версия семь" }] } }).catch(() => {});
  });
  await page.getByRole("button", { name: "Реплики", exact: true }).click();
  await expect(page.getByText("Исходная версия", { exact: true })).toBeVisible();
  await page.clock.runFor(3100);
  await expect.poll(() => calls).toBe(2);
  await page.locator(".ver-switch select").last().selectOption("7");
  await expect.poll(() => aborted.some(path => path.endsWith("/transcript"))).toBe(true);
  await expect(page.getByText("Выбранная версия семь", { exact: true })).toBeVisible();
  release();
  await expect(page.getByText("Устаревшее обновление версии", { exact: true })).toHaveCount(0);
});

test("pipeline keeps elapsed time, the next step and an explicit reason between workers and retries", async ({ page }) => {
  await workspace(page);
  let state = "queued";
  let offline = false;
  const stage = (name: string, status: string, started_at?: string, ended_at?: string) => ({ stage: name, state: status, started_at, ended_at, updated_at: ended_at ?? started_at });
  await page.route("**/progress", route => {
    if (offline) return route.fulfill({ status: 503, json: { error: "synthetic progress outage" } });
    const current = state === "running" || state === "running_retry";
    const summary = state === "queued" ? stage("summary", "queued") : state === "retry_wait" ? stage("summary", "failed", "2026-10-31T07:00:25Z", "2026-10-31T07:00:30Z") : stage("summary", current ? "running" : state, state === "running" ? "2026-10-31T07:00:25Z" : "2026-10-31T08:00:30Z", state === "done" ? "2026-10-31T08:00:40Z" : undefined);
    const since = state === "queued" ? "2026-10-31T07:00:20Z" : state === "retry_wait" ? "2026-10-31T07:00:30Z" : state === "running" ? "2026-10-31T07:00:25Z" : "2026-10-31T08:00:30Z";
    return route.fulfill({ json: { running: current || state === "queued", stages: [
      stage("transcribe", "done", "2026-10-31T07:00:01Z", "2026-10-31T07:00:10Z"),
      stage("refine", "skipped"), stage("cleanup", "skipped"), summary,
    ], execution: { run: "r1", operation: state === "running_retry" ? "op2" : "op1", state: current ? "running" : state, phase: "summary", current: current ? "summary" : null, since, next: state === "queued" || state === "retry_wait" ? ["summary"] : [], retry_at: state === "retry_wait" ? "2026-10-31T08:00:30Z" : null,
      message: state === "queued" ? "Ожидает свободного обработчика" : current ? "Этап выполняется" : state === "retry_wait" ? "Модель недоступна; ожидается повтор" : "Все запрошенные результаты готовы" } } });
  });
  await page.getByRole("button", { name: "Ход", exact: true }).click();
  await expect(page.getByText("Ожидает свободного обработчика", { exact: true })).toBeVisible();
  await expect(page.getByText(/Ожидание: 5 с/)).toBeVisible();
  await expect(page.locator(".journal-state").first()).toHaveText("завершение не зафиксировано");
  await expect(page.locator(".step.done").first()).toContainText("за 9 с");
  await expect(page.locator(".doc")).toHaveScreenshot("pipeline-queued.png");
  await page.clock.runFor(2100);
  await expect(page.getByText(/Ожидание: 7 с/)).toBeVisible();
  state = "running";
  await page.clock.runFor(3100);
  await expect(page.getByText("Этап выполняется", { exact: true })).toBeVisible();
  await expect(page.locator(".step.running")).toHaveCount(1);
  await expect(page.locator(".journal-state").first()).toHaveText("идёт");
  offline = true;
  await page.clock.runFor(3100);
  await expect(page.getByText(/Текущее выполнение не подтверждено/)).toBeVisible();
  const frozen = await page.locator(".step.running .t .hint").innerText();
  await page.clock.runFor(10_000);
  await expect(page.locator(".step.running .t .hint")).toHaveText(frozen);
  await expect(page.locator(".journal-state").first()).toHaveText("завершение не зафиксировано");
  offline = false;
  state = "retry_wait";
  await page.clock.runFor(3100);
  await expect(page.getByText("Модель недоступна; ожидается повтор", { exact: true })).toBeVisible();
  await expect(page.getByText(/Повтор не раньше/)).toBeVisible();
  await expect(page.locator(".step.running")).toHaveCount(0);
  state = "running_retry";
  await page.clock.fastForward(3_600_000);
  await page.clock.runFor(3100);
  await expect(page.locator(".step.running")).toHaveCount(1);
  await expect(page.locator(".journal-state").first()).toHaveText("завершение не зафиксировано"); // old op1 is not retry op2
  state = "done";
  await page.clock.runFor(7100);
  await expect(page.getByText("Все запрошенные результаты готовы", { exact: true })).toBeVisible();
  await expect(page.locator(".step.done").last()).toContainText("за 10 с");
  await page.clock.runFor(60_000);
  await expect(page.locator(".step.done").last()).toContainText("за 10 с");
});


test("settings keep edits after failure, save only changed keys and show restart", async ({ page }) => {
  await workspace(page);
  const fields = [
    { key: "LOCALVOX_LLM_MODEL", label: "Модель", kind: "text", value: "before", applies: "restart" },
    { key: "LOCALVOX_LIGHT_YT_FFMPEG", label: "ffmpeg", kind: "path", value: null, applies: "restart" },
    { key: "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY", label: "Делать сводку", kind: "bool", value: "on", applies: "restart" },
    { key: "LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC", label: "Интервал", kind: "number", value: "5", applies: "restart" },
    { key: "LOCALVOX_API_TOKEN", label: "Токен API", kind: "secret", value: null, applies: "restart", set: true },
    { key: "LOCALVOX_SUMMARY_PROVIDER", label: "Провайдер", kind: "text", value: "ollama", applies: "restart", options: [{ value: "ollama", label: "Локальный" }, { value: "claude", label: "Claude" }] },
  ].map((s) => ({ group: "Параметры", hint: "по умолчанию", set: s.value != null, ...s }));
  let fail = true; const sent: Record<string, string | null>[] = [];
  await page.route("**/api/settings", async (route) => {
    if (route.request().method() === "GET") return route.fulfill({ json: { path: "synthetic/.env", exists: true, settings: fields, extra: [] } });
    const { set } = route.request().postDataJSON(); sent.push(set);
    if (fail) return route.fulfill({ status: 503, json: { error: "synthetic save failure" } });
    return route.fulfill({ json: { msg: "Сохранено", need_restart: ["Модель", "Интервал"] } });
  });
  await page.getByRole("button", { name: /⚙ Настройки/ }).click();
  const row = (key: string) => page.locator(".set-row").filter({ has: page.getByText(key, { exact: true }) });
  await row("LOCALVOX_LLM_MODEL").locator("input").fill("after");
  await row("LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC").locator("input").fill("7");
  await row("LOCALVOX_LIGHT_AUTOCOOK_SUMMARY").locator("select").selectOption("off");
  await row("LOCALVOX_SUMMARY_PROVIDER").locator("select").selectOption("claude");
  await row("LOCALVOX_API_TOKEN").locator("input").fill("synthetic-new-token");
  await expect(row("LOCALVOX_API_TOKEN").locator("input")).toHaveAttribute("type", "password");
  await expect(page.getByRole("button", { name: "Сохранить (5)", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Сохранить (5)", exact: true }).click();
  await expect(page.getByText("synthetic save failure", { exact: true })).toBeVisible();
  await expect(row("LOCALVOX_LLM_MODEL").locator("input")).toHaveValue("after");
  fail = false;
  await page.getByRole("button", { name: "Сохранить (5)", exact: true }).click();
  await expect(page.getByText(/Применится после перезапуска демона: Модель, Интервал/)).toBeVisible();
  expect(sent).toHaveLength(2); expect(sent[1]).toEqual(sent[0]);
  expect(sent[1]).toEqual({ LOCALVOX_LLM_MODEL: "after", LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC: "7", LOCALVOX_LIGHT_AUTOCOOK_SUMMARY: "off", LOCALVOX_SUMMARY_PROVIDER: "claude", LOCALVOX_API_TOKEN: "synthetic-new-token" });
  await row("LOCALVOX_LLM_MODEL").locator("input").fill("");
  await page.getByRole("button", { name: "Сохранить (1)", exact: true }).click();
  await expect.poll(() => sent.length).toBe(3);
  expect(sent[2]).toEqual({ LOCALVOX_LLM_MODEL: null });
});

test("unknown settings from old daemons never expose their values and can be unset explicitly", async ({ page }) => {
  await workspace(page);
  let sent: unknown;
  await page.route("**/api/settings", (route) => {
    if (route.request().method() !== "GET") { sent = route.request().postDataJSON(); return route.fulfill({ json: { msg: "Сохранено", need_restart: ["LOCALVOX_FUTURE_TOKEN"] } }); }
    return route.fulfill({ json: { ...fixture.settings, extra: [{ key: "LOCALVOX_FUTURE_TOKEN", value: "synthetic-must-not-render", set: true }] } });
  });
  await page.getByRole("button", { name: /⚙ Настройки/ }).click();
  await page.getByRole("button", { name: /Остальное в файле/ }).click();
  await expect(page.getByLabel("Новое значение LOCALVOX_FUTURE_TOKEN")).toHaveValue("");
  await expect(page.locator("body")).not.toContainText("synthetic-must-not-render");
  await page.locator(".set-row").filter({ has: page.getByText("LOCALVOX_FUTURE_TOKEN", { exact: true }) }).getByRole("button", { name: "Сбросить", exact: true }).click();
  await page.getByRole("button", { name: "Сохранить (1)", exact: true }).click();
  await expect.poll(() => sent).toEqual({ set: { LOCALVOX_FUTURE_TOKEN: null } });
});

test("browser access remains editable after 401 and uses a masked local token", async ({ page }) => {
  await workspace(page);
  let latest = "";
  await page.route("**/api/settings", (route) => {
    latest = route.request().headers().authorization ?? "";
    return latest === "Bearer synthetic-browser-token" ? route.fulfill({ json: fixture.settings }) : route.fulfill({ status: 401, json: { error: "unauthorized" } });
  });
  await page.getByRole("button", { name: /⚙ Настройки/ }).click();
  await expect(page.getByText("нет доступа — задайте токен", { exact: true })).toBeVisible();
  const token = page.getByLabel("Новый токен доступа");
  await token.fill("synthetic-browser-token");
  await expect(token).toHaveAttribute("type", "password");
  await page.getByRole("button", { name: "Подключиться", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Настройки", exact: true })).toBeVisible();
  expect(latest).toBe("Bearer synthetic-browser-token");
  await expect(page.locator("body")).not.toContainText("synthetic-browser-token");
  await page.getByRole("button", { name: "Очистить токен доступа", exact: true }).click();
  await expect(page.getByText("нет доступа — задайте токен", { exact: true })).toBeVisible();
  expect(latest).toBe("");
});

test("document actions use real version IDs, explicit language and all three retry scopes", async ({ page }) => {
  await workspace(page);
  const mutations: { path: string; body: unknown }[] = [];
  await page.route("**/api/**", (route) => {
    const request = route.request(); const url = new URL(request.url());
    if (request.method() !== "GET") { mutations.push({ path: url.pathname + url.search, body: request.postDataJSON() }); return route.fulfill({ json: { ok: true, best: 2, msg: "Сохранено" } }); }
    if (url.pathname.endsWith("/versions")) return route.fulfill({ json: { versions: [...fixture.versions, { ...fixture.versions[0], id: 7, best: false }] } });
    return route.fallback();
  });
  await page.getByRole("button", { name: "Реплики", exact: true }).click();
  await expect(page.getByText("Давайте отделим интерфейс от движка.", { exact: true })).toBeVisible();
  await page.locator(".ver-switch select").last().selectOption("7");
  await expect.poll(() => mutations.length).toBe(1);
  expect(mutations[0]).toEqual({ path: "/api/sessions/20261031_demo/best", body: { id: 7 } });
  page.once("dialog", (dialog) => dialog.dismiss());
  await page.getByLabel("Язык:", { exact: true }).selectOption("en");
  expect(mutations).toHaveLength(1);
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByLabel("Язык:", { exact: true }).selectOption("en");
  await expect.poll(() => mutations.length).toBe(2);
  expect(mutations[1]).toEqual({ path: "/api/sessions/20261031_demo/lang", body: { lang: "en" } });
  for (const [tab, scope] of [["Сводка", "summary"], ["Текст", "text"], ["Участники", "all"]]) {
    await page.getByRole("button", { name: tab, exact: true }).click();
    page.once("dialog", (dialog) => dialog.accept());
    await page.locator(".doc-head").getByRole("button", { name: /Переварить/ }).click();
    await expect.poll(() => mutations.at(-1)?.path).toBe(`/api/sessions/20261031_demo/recook?scope=${scope}`);
  }
});

test("participant and source names remain session local and support an empty source reset", async ({ page }) => {
  await workspace(page);
  const sent: unknown[] = [];
  const people = [...fixture.speakers, { name: "Звук видео", what: "звук из источника", named: true, voices: [], sources: [1], speech_sec: 3, lines: 1 }];
  await page.route("**/api/sessions/*/speakers", (route) => {
    if (route.request().method() === "GET") return route.fulfill({ json: { speakers: people } });
    sent.push(route.request().postDataJSON()); return route.fulfill({ json: { msg: "Имя сохранено только в записи" } });
  });
  await page.route("**/api/sessions/*/sources", (route) => { sent.push(route.request().postDataJSON()); return route.fulfill({ json: { msg: "Источник сохранён" } }); });
  await page.getByRole("button", { name: "Участники", exact: true }).click();
  page.once("dialog", (dialog) => { expect(dialog.message()).toContain("только в этой записи"); return dialog.accept("Мария 🦀"); });
  await page.getByRole("button", { name: "✎ Назвать", exact: true }).click();
  await expect.poll(() => sent.length).toBe(2);
  expect(sent).toEqual([{ speaker: "speaker_0", name: "Мария 🦀" }, { source_id: 0, name: "Мария 🦀" }]);
  page.once("dialog", (dialog) => dialog.accept(""));
  await page.getByRole("button", { name: "✎ Переименовать", exact: true }).click();
  await expect.poll(() => sent.length).toBe(3);
  expect(sent[2]).toEqual({ source_id: 1, name: "" });
});

test("clean document copy omits provenance and session deletion requires a second click", async ({ page }) => {
  await workspace(page);
  let copied = ""; await page.exposeFunction("captureClipboard", (text: string) => { copied = text; });
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: { writeText: (text: string) => (window as unknown as { captureClipboard: (text: string) => Promise<void> }).captureClipboard(text) } }));
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  await expect(page.locator(".book")).toContainText("Отделим интерфейс от движка.");
  await page.getByText("Исходные реплики (1)", { exact: true }).click();
  await expect(page.locator(".doc details")).toContainText("Давайте отделим интерфейс от движка.");
  await page.locator(".doc-head").getByRole("button", { name: /Копировать/ }).click();
  await expect.poll(() => copied).toContain("Отделим интерфейс от движка.");
  expect(copied).not.toMatch(/поправлено|таймкодов|модель|Давайте/);
  const deleted: unknown[] = [];
  await page.route("**/api/sessions/20261031_demo", (route) => { deleted.push(route.request().postDataJSON()); return route.fulfill({ json: { msg: "Удалено" } }); });
  await page.getByTitle("Удалить запись вместе со звуком").click();
  await page.getByRole("button", { name: "Отмена", exact: true }).click(); expect(deleted).toHaveLength(0);
  await page.getByTitle("Удалить запись вместе со звуком").click();
  await page.getByRole("button", { name: "Удалить", exact: true }).click();
  await expect.poll(() => deleted).toEqual([{ confirm: "20261031_demo" }]);
});

test("notes retain drafts on delivery failure, reject stale route hints and delete by raw source", async ({ page }) => {
  await workspace(page);
  const slots = structuredClone(fixture.slots); slots.push({ name: "другое", default: false, readable: false, error: "сюда можно только писать", notes: [] });
  const writes: unknown[] = []; const deletes: unknown[] = [];
  let releaseRoute!: () => void; const routeGate = new Promise<void>((resolve) => { releaseRoute = resolve; }); let routeCalls = 0;
  let releaseSave!: () => void; const saveGate = new Promise<void>((resolve) => { releaseSave = resolve; });
  await page.route("**/api/slots/notes**", (route) => {
    if (route.request().method() === "GET") return route.fulfill({ json: { slots } });
    deletes.push(route.request().postDataJSON()); slots[0].notes = []; return route.fulfill({ json: { msg: "Удалено" } });
  });
  await page.route("**/api/notes", async (route) => {
    writes.push(route.request().postDataJSON());
    if (writes.length === 1) return route.fulfill({ status: 503, json: { error: "synthetic delivery failure" } });
    await saveGate; return route.fulfill({ json: { msg: "Записано" } });
  });
  await page.route("**/api/route", async (route) => {
    routeCalls++; if (routeCalls === 1) await routeGate;
    return route.fulfill({ json: { suggestions: [{ slot: "другое", reason: "По теме" }] } });
  });
  await page.getByRole("button", { name: /📝 Заметки/ }).click();
  const input = page.getByPlaceholder("Записать в «идеи»…");
  await input.fill("Первый текст"); await page.getByTitle("Куда занести? Спросить модель").click();
  await expect.poll(() => routeCalls).toBe(1); await input.fill("Исправленный текст 🦀"); releaseRoute();
  await expect(page.getByTitle("Куда занести? Спросить модель")).toBeEnabled();
  await expect(page.getByTitle("По теме", { exact: true })).toHaveCount(0); expect(writes).toHaveLength(0);
  await page.getByRole("button", { name: "Записать", exact: true }).click();
  await expect(page.getByText("synthetic delivery failure", { exact: true })).toBeVisible();
  await expect(input).toHaveValue("Исправленный текст 🦀");
  await page.getByRole("button", { name: "Записать", exact: true }).click();
  await expect.poll(() => writes.length).toBe(2); await input.fill("Следующая заметка"); releaseSave();
  await expect(page.getByRole("button", { name: "Записать", exact: true })).toBeEnabled();
  await expect(input).toHaveValue("Следующая заметка");
  expect(writes[1]).toEqual({ text: "Исправленный текст 🦀", slot: "идеи" });
  await page.getByTitle("Удалить заметку", { exact: true }).click(); await page.getByRole("button", { name: "Отмена", exact: true }).click(); expect(deletes).toHaveLength(0);
  await page.getByTitle("Удалить заметку", { exact: true }).click(); await page.getByRole("button", { name: "Удалить", exact: true }).click();
  await expect.poll(() => deletes).toEqual([{ slot: "идеи", raw: "- Проверить очередь", source: "synthetic-notes.md" }]);
  await expect(page.getByText("Здесь пока ничего не записано.", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "другое", exact: true }).click();
  await expect(page.getByText("Не показать содержимое: сюда можно только писать", { exact: true })).toBeVisible();
  await expect(page.getByText("Здесь пока ничего не записано.", { exact: true })).not.toBeVisible();
});

test("asks send explicit providers and Unicode files once, retain failures and return to saved history", async ({ page }) => {
  await workspace(page);
  let record = { ...fixture.ask, status: "pending", answer: null, input_name: "Материал 🦀.txt", input: "Текст из файла 🦀" };
  const sent: unknown[] = []; const deleted: string[] = []; let reject = true;
  await page.route("**/api/asks", (route) => {
    if (route.request().method() === "GET") return route.fulfill({ json: { asks: [{ ...fixture.asks[0], status: record.status }] } });
    sent.push(route.request().postDataJSON());
    return reject ? route.fulfill({ status: 503, json: { error: "synthetic ask failure" } }) : route.fulfill({ json: record });
  });
  await page.route("**/api/asks/demo-ask", (route) => {
    if (route.request().method() === "DELETE") { deleted.push("demo-ask"); return route.fulfill({ json: { msg: "Удалено" } }); }
    return route.fulfill({ json: record });
  });
  await page.getByRole("button", { name: /💬 Спросить/ }).click();
  await page.locator('input[type="file"]').setInputFiles({ name: "Материал 🦀.txt", mimeType: "text/plain", buffer: Buffer.from("Текст из файла 🦀", "utf8") });
  await page.locator(".ask-actions select").selectOption("ollama");
  await page.getByPlaceholder(/Задача \(необязательно\)/).fill("Сделай выжимку");
  await page.getByRole("button", { name: "Спросить", exact: true }).click();
  await expect(page.getByText("synthetic ask failure", { exact: true })).toBeVisible();
  await expect(page.getByPlaceholder(/Вставьте текст/)).toHaveValue("Текст из файла 🦀");
  reject = false; await page.getByRole("button", { name: "Спросить", exact: true }).click();
  await expect(page.getByText(/Ответ готовится…/)).toBeVisible();
  expect(sent).toHaveLength(2); expect(sent[1]).toEqual({ text: "Текст из файла 🦀", prompt: "Сделай выжимку", provider: "ollama", input_name: "Материал 🦀.txt" });
  await page.getByRole("button", { name: /📝 Заметки/ }).click();
  record = { ...record, status: "done", answer: "Готовый ответ 🦀" };
  await page.getByRole("button", { name: /💬 Спросить/ }).click();
  await expect(page.getByText("Готовый ответ 🦀", { exact: true })).toBeVisible(); expect(sent).toHaveLength(2);
  let copied = ""; await page.exposeFunction("copyAsk", (text: string) => { copied = text; });
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: { writeText: (text: string) => (window as unknown as { copyAsk: (text: string) => Promise<void> }).copyAsk(text) } }));
  await page.getByRole("button", { name: "⧉ Копировать ответ", exact: true }).click(); await expect.poll(() => copied).toBe("Готовый ответ 🦀");
  await page.locator(".ask-input").getByRole("button", { name: "⧉ Копировать", exact: true }).click(); await expect.poll(() => copied).toBe("Текст из файла 🦀");
  await page.getByTitle("Удалить вопрос", { exact: true }).click(); await page.getByRole("button", { name: "Отмена", exact: true }).click(); expect(deleted).toHaveLength(0);
  await page.getByTitle("Удалить вопрос", { exact: true }).click(); await page.getByRole("button", { name: "Удалить", exact: true }).click();
  await expect.poll(() => deleted).toEqual(["demo-ask"]);
});

test("search modes encode the Unicode query and grounded chat opens its source", async ({ page }) => {
  await workspace(page); const searches: { query: string | null; mode: string | null }[] = [];
  await page.route("**/api/search**", (route) => {
    const url = new URL(route.request().url()); searches.push({ query: url.searchParams.get("q"), mode: url.searchParams.get("mode") });
    return route.fulfill({ json: [{ session: "20261031_demo", kind: "transcript", start_sec: 3, snippet: "Найденная реплика 🦀", why: "слова + смысл" }] });
  });
  const query = page.getByPlaceholder("Поиск по архиву…");
  await query.press("Enter"); expect(searches).toHaveLength(0);
  await query.fill("архитектура & 🦀");
  for (const mode of ["", "semantic", "hybrid"]) {
    await page.getByTitle("Режим поиска").selectOption(mode); await query.press("Enter");
    await expect.poll(() => searches.at(-1)?.mode).toBe(mode || null);
  }
  expect(searches.map((s) => s.query)).toEqual(Array(3).fill("архитектура & 🦀"));
  await page.getByRole("button", { name: /Найденная реплика 🦀/ }).click();
  await expect(page.getByRole("button", { name: "Реплики", exact: true })).toHaveAttribute("aria-selected", "true");
  await page.route("**/api/ask", (route) => { expect(route.request().postDataJSON()).toEqual({ question: "Что решили?" }); return route.fulfill({ json: { text: "Решили разделить клиент.", verified: true, sources: [{ n: 1, session: "20261031_demo", kind: "transcript", start_sec: 3, text: "Источник решения" }] } }); });
  await page.getByPlaceholder("Вопрос по записям…").fill("Что решили?"); await page.getByPlaceholder("Вопрос по записям…").press("Enter");
  await expect(page.getByText("Решили разделить клиент.", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /Источник решения/ }).click();
  await expect(page.getByRole("button", { name: "Реплики", exact: true })).toHaveAttribute("aria-selected", "true");
});

test("recorder keeps stop available before confirmation and imports open their accepted session", async ({ page }) => {
  await workspace(page);
  let active = false; let confirmStart = false; let imported = false;
  const controls: { path: string; body: unknown }[] = [];
  await page.route("**/api/record**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (route.request().method() === "POST") {
      controls.push({ path, body: route.request().postDataJSON() });
      active = path.endsWith("/start") && confirmStart;
      return route.fulfill({ json: { msg: "Принято", session: "20261031_demo" } });
    }
    return route.fulfill({ json: { ...fixture.record, recording: active, session: active ? "20261031_demo" : null } });
  });
  page.once("dialog", (dialog) => dialog.accept("Тест записи 🦀"));
  await page.getByRole("button", { name: "🎙 С названием…", exact: true }).click();
  await expect(page.getByText("запись включается…", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Остановить", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Остановить", exact: true }).click();
  await expect(page.getByRole("button", { name: "Запись", exact: true })).toBeVisible();
  expect(controls[0]).toEqual({ path: "/api/record/start", body: { title: "Тест записи 🦀" } });
  expect(controls[1]).toEqual({ path: "/api/record/stop", body: {} });
  await page.clock.runFor(1100); confirmStart = true;
  await page.getByRole("button", { name: "Запись", exact: true }).click();
  await expect(page.getByRole("button", { name: "Остановить", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Остановить", exact: true }).click();
  await expect(page.getByRole("button", { name: "Запись", exact: true })).toBeVisible();
  await page.route("**/api/sessions?**", (route) => route.fulfill({ json: imported ? [...fixture.sessions, { ...fixture.sessions[0], name: "accepted-new", title: "Импорт принят" }] : fixture.sessions }));
  await page.route("**/api/ingest", (route) => { expect(route.request().postDataJSON()).toEqual({ url: "https://example.org/новая-встреча" }); imported = true; return route.fulfill({ json: { session: "accepted-new" } }); });
  await page.getByPlaceholder("Поиск по архиву…").fill("https://example.org/новая-встреча");
  await page.getByRole("button", { name: "↓ Расшифровать по ссылке", exact: true }).click();
  await expect(page.getByRole("heading", { name: /^Импорт принят/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Ход", exact: true })).toHaveAttribute("aria-selected", "true");
});

test("artifact confirmation exposes integrity errors and provenance is excluded from copies", async ({ page }) => {
  await workspace(page);
  let confirmed = false; const sent: unknown[] = [];
  await page.route("**/api/sessions?**", (route) => route.fulfill({ json: [{ ...fixture.sessions[0], summary_doubts: confirmed ? null : "Проверить утверждение" }] }));
  await page.route("**/api/sessions/*/summary", (route) => route.fulfill({ json: { ...fixture.summary, provenance: { model: "synthetic-model", template: "synthetic-template", glossary: 0, at: "2026-10-31T07:00:00Z" } } }));
  await page.route("**/api/sessions/*/confirm", (route) => {
    sent.push(route.request().postDataJSON());
    if (sent.length === 1) return route.fulfill({ status: 400, json: { error: "synthetic checksum mismatch" } });
    confirmed = true; return route.fulfill({ json: { msg: "Подтверждено" } });
  });
  await page.reload();
  await expect(page.getByText("synthetic-model", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "✓ Всё верно", exact: true }).click();
  await expect(page.getByText("synthetic checksum mismatch", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "✓ Всё верно", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "✓ Всё верно", exact: true }).click();
  await expect(page.getByRole("button", { name: "✓ Всё верно", exact: true })).not.toBeVisible();
  expect(sent).toEqual([{ artifact: "summary" }, { artifact: "summary" }]);
  let copied = ""; await page.exposeFunction("captureCopy", (text: string) => { copied = text; });
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: { writeText: (text: string) => (window as unknown as { captureCopy: (text: string) => Promise<void> }).captureCopy(text) } }));
  await page.locator(".doc-head").getByRole("button", { name: /Копировать/ }).click();
  await expect.poll(() => copied).toContain("Разделить клиент"); expect(copied).not.toMatch(/synthetic-model|synthetic-template|Проверить утверждение/);
});

async function realAudio(page: Page, seconds = 120) {
  const bytes = Buffer.alloc(44 + seconds * 16000 * 2);
  bytes.write("RIFF", 0); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(16000, 24); bytes.writeUInt32LE(32000, 28); bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36); bytes.writeUInt32LE(bytes.length - 44, 40);
  await page.route("**/audio.wav**", (route) => {
    const range = /^bytes=(\d+)-(\d*)$/.exec(route.request().headers().range ?? "");
    const from = Number(range?.[1] ?? 0), end = Math.min(bytes.length - 1, range?.[2] ? Number(range[2]) : bytes.length - 1);
    return route.fulfill({ status: range ? 206 : 200, contentType: "audio/wav", headers: { "Accept-Ranges": "bytes", ...(range ? { "Content-Range": `bytes ${from}-${end}/${bytes.length}` } : {}) }, body: bytes.subarray(from, end + 1) });
  });
  // The initial fixture deliberately had no audio. Reload the element after
  // installing real PCM; browser decoding is real, the HTTP is intercepted.
  const audio = page.locator("audio"); await audio.evaluate((node: HTMLAudioElement) => node.load());
  await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.readyState)).toBeGreaterThan(0);
  return audio;
}

test("native browser audio supports pause, bounded keyboard seek and repeated fragments", async ({ page }) => {
  await workspace(page);
  const audio = await realAudio(page);
  await page.getByTitle("Слушать", { exact: true }).click(); await expect(page.getByTitle("Пауза", { exact: true })).toBeVisible();
  await page.getByTitle("Пауза", { exact: true }).click();
  await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.paused)).toBe(true);
  await expect(page.getByTitle("Слушать", { exact: true })).toBeVisible();
  const slider = page.getByRole("slider", { name: "Перемотка", exact: true }); await slider.focus(); await slider.press("End");
  await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.currentTime)).toBeGreaterThan(119);
  await slider.press("Home"); await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.currentTime)).toBe(0);
  await slider.press("ArrowRight"); await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.currentTime)).toBe(5);
  await page.getByTitle("Вперёд 15 с", { exact: true }).click(); await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.currentTime)).toBe(20);
  await page.getByRole("button", { name: "Реплики", exact: true }).click();
  const fragment = page.getByTitle("Послушать этот фрагмент (00:00–00:10)", { exact: true });
  for (let i = 0; i < 2; i++) {
    await fragment.click(); await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.currentTime)).toBeLessThan(2);
    await audio.evaluate((node: HTMLAudioElement) => { node.currentTime = 10.1; node.dispatchEvent(new Event("timeupdate")); });
    await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.paused)).toBe(true);
  }
});

test("archive metadata refresh preserves the paused audio position without reloading", async ({ page }) => {
  await workspace(page);
  const audio = await realAudio(page);
  await audio.evaluate((node: HTMLAudioElement) => {
    node.currentTime = 20; node.dispatchEvent(new Event("timeupdate"));
    const load = node.load.bind(node);
    node.load = () => { node.dataset.reloads = String(Number(node.dataset.reloads ?? 0) + 1); load(); };
  });
  await page.route("**/api/sessions?**", route => route.fulfill({ json: fixture.sessions.map((s: object) => ({ ...s, duration_sec: 180 })) }));
  await page.clock.runFor(5100);
  await expect(page.getByRole("slider", { name: "Перемотка", exact: true })).toHaveAttribute("aria-valuemax", "180");
  await expect(audio).toHaveCount(1);
  expect(await audio.evaluate((node: HTMLAudioElement) => ({ time: node.currentTime, paused: node.paused, reloads: Number(node.dataset.reloads ?? 0) }))).toEqual({ time: 20, paused: true, reloads: 0 });
});

test("archive metadata refresh preserves playing audio and its fragment boundary", async ({ page }) => {
  await workspace(page);
  const audio = await realAudio(page);
  await page.route("**/transcript**", route => route.fulfill({ json: { lines: [{ ...fixture.transcript.lines[0], start_sec: 30, end_sec: 110 }] } }));
  await page.getByRole("button", { name: "Реплики", exact: true }).click();
  await page.getByTitle("Послушать этот фрагмент (00:30–01:50)", { exact: true }).click();
  await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.paused)).toBe(false);
  await page.route("**/api/sessions?**", route => route.fulfill({ json: fixture.sessions.map((s: object) => ({ ...s, duration_sec: 180 })) }));
  await page.clock.runFor(5100);
  await expect(page.getByRole("slider", { name: "Перемотка", exact: true })).toHaveAttribute("aria-valuemax", "180");
  expect(await audio.evaluate((node: HTMLAudioElement) => node.paused)).toBe(false);
  expect(await audio.evaluate((node: HTMLAudioElement) => node.currentTime)).toBeGreaterThanOrEqual(30);
  expect(await page.locator(".wave i.seg").count()).toBeGreaterThan(0);
  await audio.evaluate((node: HTMLAudioElement) => { node.currentTime = 110.1; node.dispatchEvent(new Event("timeupdate")); });
  await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.paused)).toBe(true);
  expect(await page.locator(".wave i.seg-on").count()).toBeGreaterThan(0);
});

for (const change of ["session", "view", "availability"] as const) {
  test(`audio ${change} change stops the previous source and never replays its old fragment`, async ({ page }) => {
    await workspace(page);
    const audio = await realAudio(page);
    const previous = await audio.elementHandle();
    let duration = 120;
    await page.route("**/api/sessions?**", route => route.fulfill({ json: [
      { ...fixture.sessions[0], duration_sec: duration },
      { ...fixture.sessions[0], name: "session-b", title: "Другая запись" },
    ] }));
    await page.clock.runFor(5100);
    await page.route("**/transcript**", route => route.fulfill({ json: { lines: [{ ...fixture.transcript.lines[0], start_sec: 30, end_sec: 110 }] } }));
    await page.getByRole("button", { name: "Реплики", exact: true }).click();
    await page.getByTitle("Послушать этот фрагмент (00:30–01:50)", { exact: true }).click();
    await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.paused)).toBe(false);
    if (change === "session") {
      await page.locator(".side .item").filter({ hasText: "Другая запись" }).click();
      await expect(audio).toHaveAttribute("src", /session-b\/audio\.wav/);
    } else {
      if (change === "view") await page.locator(".nav").getByRole("button", { name: /Заметки/ }).click();
      else { duration = 0; await page.clock.runFor(5100); }
      await expect(audio).toHaveCount(0);
      expect(await previous!.evaluate(node => ({ paused: node.paused, src: node.getAttribute("src") }))).toEqual({ paused: true, src: null });
      if (change === "view") await page.locator(".nav").getByRole("button", { name: /Записи/ }).click();
      else { duration = 120; await page.clock.runFor(5100); }
    }
    await expect(audio).toHaveCount(1);
    await expect.poll(() => audio.evaluate((node: HTMLAudioElement) => node.readyState)).toBeGreaterThan(0);
    expect(await audio.evaluate((node: HTMLAudioElement) => ({ paused: node.paused, time: node.currentTime }))).toEqual({ paused: true, time: 0 });
    await expect(page.locator(".wave i.seg, .wave i.seg-on")).toHaveCount(0);
    await expect(page.getByRole("slider", { name: "Перемотка", exact: true })).toHaveAttribute("aria-valuenow", "0");
    await previous?.dispose();
  });
}

test("waveform refresh coalesces slow reads, keeps its shape and cancels a superseded session", async ({ page }) => {
  await workspace(page);
  await expect(page.locator(".wave i").first()).toHaveCSS("height", "3px");
  const aborted = await watchReadAborts(page, ["peaks"]);
  let duration = 180, calls = 0, served = 0;
  let release!: () => void; const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/sessions?**", route => route.fulfill({ json: [
    { ...fixture.sessions[0], duration_sec: duration },
    { ...fixture.sessions[0], name: "session-b", title: "Другая волна" },
  ] }));
  await page.route("**/20261031_demo/peaks?**", async route => {
    calls++; await gate;
    await route.fulfill({ json: { duration_sec: duration, peaks: Array(320).fill(0.9) } }).catch(() => {}); served++;
  });
  await page.route("**/session-b/peaks?**", route => route.fulfill({ json: { duration_sec: 120, peaks: Array(320).fill(0.5) } }));
  await page.clock.runFor(5100); await expect.poll(() => calls).toBe(1);
  duration = 210; await page.clock.runFor(5100);
  await expect(page.getByRole("slider", { name: "Перемотка", exact: true })).toHaveAttribute("aria-valuemax", "210");
  duration = 240; await page.clock.runFor(5100);
  await expect(page.getByRole("slider", { name: "Перемотка", exact: true })).toHaveAttribute("aria-valuemax", "240");
  expect(calls).toBe(1);
  await expect(page.locator(".wave i").first()).toHaveCSS("height", "3px");
  release(); await expect.poll(() => calls).toBe(2); await expect.poll(() => served).toBe(2);
  await expect(page.locator(".wave i").first()).toHaveCSS("height", "31px");
  const nextGate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/20261031_demo/peaks?**", async route => {
    calls++; await nextGate;
    await route.fulfill({ json: { duration_sec: 270, peaks: Array(320).fill(0.9) } }).catch(() => {}); served++;
  });
  duration = 270; await page.clock.runFor(5100); await expect.poll(() => calls).toBe(3);
  await page.locator(".side .item").filter({ hasText: "Другая волна" }).click();
  await expect.poll(() => aborted.includes("/api/sessions/20261031_demo/peaks")).toBe(true);
  await expect(page.locator(".wave i").first()).toHaveCSS("height", "17px");
  release(); await expect.poll(() => served).toBe(3);
  await expect(page.locator(".wave i").first()).toHaveCSS("height", "17px");
  expect(calls).toBe(3);
});

test("failed waveform refresh retains the last shape and playback and can recover", async ({ page }) => {
  await workspace(page);
  const audio = await realAudio(page);
  await expect(page.locator(".wave i").first()).toHaveCSS("height", "3px");
  await audio.evaluate((node: HTMLAudioElement) => { node.currentTime = 20; node.dispatchEvent(new Event("timeupdate")); });
  let duration = 180, calls = 0;
  let release!: () => void; const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/sessions?**", route => route.fulfill({ json: [{ ...fixture.sessions[0], duration_sec: duration }] }));
  await page.route("**/peaks?**", async route => {
    calls++;
    if (calls === 1) return route.fulfill({ status: 503, json: { error: "synthetic waveform failure" } });
    await gate;
    return route.fulfill({ json: { duration_sec: duration, peaks: Array(320).fill(0.5) } });
  });
  await page.clock.runFor(5100); await expect.poll(() => calls).toBe(1);
  duration = 210; await page.clock.runFor(5100); await expect.poll(() => calls).toBe(2);
  // The second sequential read can start only after the first error has been handled.
  await expect(page.locator(".wave i").first()).toHaveCSS("height", "3px");
  expect(await audio.evaluate((node: HTMLAudioElement) => node.currentTime)).toBe(20);
  release();
  await expect(page.locator(".wave i").first()).toHaveCSS("height", "17px");
  expect(await audio.evaluate((node: HTMLAudioElement) => ({ time: node.currentTime, paused: node.paused }))).toEqual({ time: 20, paused: true });
});

test("archive keeps its last rows on failure and ignores a late document from another session", async ({ page }) => {
  await workspace(page); let failed = false;
  const aborted = await watchReadAborts(page);
  await page.route("**/api/sessions?**", (route) => failed ? route.fulfill({ status: 503, json: { error: "synthetic archive error" } }) : route.fulfill({ json: [...fixture.sessions, { ...fixture.sessions[0], name: "session-b", title: "Другая встреча 🦀" }] }));
  let requested = 0; let release!: () => void; const gate = new Promise<void>((resolve) => { release = resolve; });
  await page.route("**/api/sessions/20261031_demo/summary", async (route) => { requested++; await gate; return route.fulfill({ json: { blocks: [{ kind: "paragraph", text: "Устаревший ответ A", sources: [] }] } }).catch(() => {}); });
  await page.route("**/api/sessions/session-b/summary", (route) => route.fulfill({ json: { blocks: [{ kind: "paragraph", text: "Актуальный документ B", sources: [] }] } }));
  await page.clock.runFor(5100);
  const beforeReopening = requested;
  await page.getByRole("button", { name: "Текст", exact: true }).click(); await page.getByRole("button", { name: "Сводка", exact: true }).click();
  await expect.poll(() => requested).toBeGreaterThan(beforeReopening);
  aborted.length = 0;
  await page.locator(".side .item").filter({ hasText: "Другая встреча 🦀" }).click();
  await expect.poll(() => aborted.some(path => path === "/api/sessions/20261031_demo/summary")).toBe(true);
  await expect(page.getByText("Актуальный документ B", { exact: true })).toBeVisible(); release();
  await expect(page.getByText("Устаревший ответ A", { exact: true })).not.toBeVisible();
  failed = true; await page.clock.runFor(5100);
  await expect(page.getByText("synthetic archive error", { exact: true })).toBeVisible();
  await expect(page.locator(".side .item").filter({ hasText: "Другая встреча 🦀" })).toBeVisible();
  await expect(page.getByText("Актуальный документ B", { exact: true })).toBeVisible();
});

test("dictation status and failed delivery stay distinct from recording", async ({ page }) => {
  await workspace(page);
  let voice: unknown = { active: true, detail: "слушает", capturing: { slot: "идеи", text: "Диктовка 🦀" } };
  await page.route("**/api/record", (route) => route.fulfill({ json: { ...fixture.record, voice } }));
  await page.clock.runFor(1100);
  await expect(page.locator(".bar")).toContainText("Диктовка 🦀");
  await expect(page.getByRole("button", { name: "Запись", exact: true })).toBeVisible();
  voice = { active: true, detail: "слушает", last: { slot: "идеи", text: "Диктовка 🦀", dest: "synthetic", at: "2026-10-31T07:00:25Z", error: "synthetic delivery error" } };
  await page.clock.runFor(1100);
  await page.locator(".vchip.bad").click();
  await expect(page.getByText(/Последняя заметка НЕ записалась: synthetic delivery error/)).toBeVisible();
});

test("archive remains readable when journal API fails", async ({ page }) => {
  await workspace(page, { journalError: true });
  await page.getByRole("button", { name: "Ход", exact: true }).click();
  await expect(page.getByText(/Журнал не обновляется/)).toBeVisible();
  await page.getByRole("button", { name: "Сводка", exact: true }).click();
  await expect(page.getByText("Разделить клиент и движок обработки.")).toBeVisible();
});

test("arrivals, filtering and network recovery preserve expanded history and reading position", async ({ page }) => {
  await workspace(page);
  let fail = false;
  const events = Array.from({ length: 35 }, (_, i) => ({ id: `log-${i}`, run: "run", operation: "operation", stage: "summary", kind: "log", at: "2026-10-31T07:00:20Z", label: "worker", text: `Строка ${i + 1}` }));
  await page.route("**/journal**", (route) => fail ? route.fulfill({ status: 503, json: { error: "temporary failure" } }) : route.fulfill({ json: { ...fixture.journal, events } }));
  await page.getByRole("button", { name: "Ход", exact: true }).click();
  await page.getByRole("button", { name: "+ 35 lines", exact: true }).click();
  const scroll = page.locator(".journal-scroll");
  await scroll.evaluate((element) => { element.scrollTop = 90; element.dispatchEvent(new Event("scroll")); });
  const position = await scroll.evaluate((element) => element.scrollTop);
  events.push({ ...events[0], id: "new", text: "Новая строка" });
  await page.clock.runFor(2600);
  await expect(page.getByRole("button", { name: /новые: 1/ })).toBeVisible();
  expect(await scroll.evaluate((element) => element.scrollTop)).toBe(position);
  await expect(page.getByRole("button", { name: "− свернуть", exact: true })).toHaveAttribute("aria-expanded", "true");
  await page.getByRole("textbox", { name: "Фильтр журнала" }).fill("совпадений нет");
  await expect(page.locator(".journal-operation")).toHaveCount(0);
  await page.getByRole("textbox", { name: "Фильтр журнала" }).fill("");
  await expect(page.getByRole("button", { name: "− свернуть", exact: true })).toHaveAttribute("aria-expanded", "true");
  fail = true;
  await page.clock.runFor(2600);
  await expect(page.getByText(/Журнал не обновляется/)).toBeVisible();
  await expect(page.getByText("Новая строка", { exact: true })).toBeVisible();
  fail = false;
  await page.clock.runFor(5100);
  await expect(page.getByText(/Журнал не обновляется/)).not.toBeVisible();
  await expect(page.getByRole("button", { name: "− свернуть", exact: true })).toHaveAttribute("aria-expanded", "true");
});

test("previous journal page preserves the current anchor and hostile HTML stays text", async ({ page }) => {
  await workspace(page);
  const entries = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => ({ id: `event-${from+i}`, run: "run", operation: `op-${from+i}`, stage: "summary", kind: "log", at: "2026-10-31T07:00:20Z", label: `operation ${from+i}`, text: "<img src=x onerror=alert(1)>" }));
  await page.route("**/journal**", (route) => { const before = new URL(route.request().url()).searchParams.has("before"); return route.fulfill({ json: { ...fixture.journal, events: before ? entries(0, 10) : entries(10, 30), truncated: !before, previous_cursor: "previous" } }); });
  await page.getByRole("button", { name: "Ход", exact: true }).click();
  const previous = page.getByRole("button", { name: "↑ Предыдущие события", exact: true });
  await previous.scrollIntoViewIfNeeded();
  const anchor = page.locator('[data-journal-id="run/op-10"]');
  // Run/stage headings move to the first older group. Anchor the actual operation row.
  const row = anchor.locator(".journal-row");
  const before = (await row.boundingBox())!.y;
  await previous.click();
  await expect(page.locator(".journal-operation")).toHaveCount(30);
  await expect.poll(async () => Math.abs((await row.boundingBox())!.y - before)).toBeLessThan(3);
  await anchor.getByRole("button", { name: "+ 1 lines", exact: true }).click();
  await expect(anchor.getByText("<img src=x onerror=alert(1)>", { exact: true })).toBeVisible();
  await expect(page.locator(".journal img")).toHaveCount(0);
});

test("phase states, queue time and heartbeats keep their server meaning", async ({ page }) => {
  await workspace(page);
  const stages = ["download", "extract", "transcribe", "refine", "cleanup", "summary"].map((stage, i) => ({ stage, state: ["queued", "blocked", "done", "skipped", "invalid", "running"][i], started_at: "2026-10-31T07:00:10Z", updated_at: "2026-10-31T07:00:20Z", done: i === 5 ? 4 : null, total: i === 5 ? 10 : null }));
  await page.route("**/progress", (route) => route.fulfill({ json: { ...fixture.progress, stages, timing: { queue_sec: 4, processing_sec: null, total_sec: null } } }));
  await page.getByRole("button", { name: "Ход", exact: true }).click();
  for (const label of ["в очереди", "ждёт предыдущий этап", "готово", "пропущено", "результат не подтверждён", "идёт"]) await expect(page.locator(".chain .st").getByText(label, { exact: true })).toBeVisible();
  await expect(page.locator(".chain").getByText("40%", { exact: true })).toBeVisible();
  await expect(page.getByText(/Ожидание старта: 4 с · После старта: — · Всего: —/)).toBeVisible();
  stages[5].updated_at = "2026-10-31T07:00:27Z";
  await page.clock.runFor(3100);
  await expect(page.locator(".chain").getByText("40%", { exact: true })).toBeVisible();
  await expect(page.locator(".chain .running")).toHaveCount(1);
  stages[5].state = "failed";
  await page.clock.runFor(3100);
  await expect(page.locator(".chain .failed")).toHaveCount(1);
  await page.getByRole("button", { name: "Реплики", exact: true }).click();
  await expect(page.getByText("Давайте отделим интерфейс от движка.", { exact: true })).toBeVisible();
});

test("slow status requests do not overlap and a queue error keeps its last rows", async ({ page }) => {
  await workspace(page);
  let calls = 0; let release!: () => void;
  const pending = new Promise<void>((resolve) => { release = resolve; });
  await page.route("**/progress", async (route) => { calls++; await pending; await route.fulfill({ json: fixture.progress }); });
  await page.getByRole("button", { name: "Ход", exact: true }).click();
  await expect.poll(() => calls).toBe(1);
  await page.clock.runFor(10_000);
  expect(calls).toBe(1); release();
  await expect(page.locator(".chain")).toBeVisible();
  let fail = false;
  await page.route("**/api/queue", (route) => fail ? route.fulfill({ status: 503, json: { error: "queue offline" } }) : route.fulfill({ json: { queue: fixture.queue } }));
  await page.getByRole("button", { name: /Очередь/ }).first().click();
  await expect(page.locator(".qrow")).toHaveCount(1);
  fail = true; await page.clock.runFor(3100);
  await expect(page.getByText(/Очередь не обновляется/)).toBeVisible();
  await expect(page.locator(".qrow")).toHaveCount(1);
  fail = false; await page.clock.runFor(6100);
  await expect(page.getByText(/Очередь не обновляется/)).toHaveCount(0);
  await expect(page.locator(".qrow")).toHaveCount(1);
});

test("an initial queue read failure is visible and never claims all work is done", async ({ page }) => {
  await workspace(page);
  let failed = true;
  const mutations: string[] = [];
  await page.route("**/api/queue", route => {
    if (route.request().method() !== "GET") mutations.push(route.request().method());
    return failed ? route.fulfill({ status: 503, json: { error: "Очередь недоступна: synthetic broken jobs.json" } })
      : route.fulfill({ json: { queue: [] } });
  });
  await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
  await expect(page.getByText("Очередь недоступна: synthetic broken jobs.json", { exact: true })).toBeVisible();
  await expect(page.getByText(/Пусто — всё обработано/)).toHaveCount(0);
  failed = false; await page.clock.runFor(6100);
  await expect(page.getByText(/Пусто — всё обработано/)).toBeVisible();
  await expect(page.getByText("Очередь недоступна: synthetic broken jobs.json", { exact: true })).toHaveCount(0);
  expect(mutations).toEqual([]);
});

test("queue retry plan reports phase, blocked backoff, exhaustion and recovery without mutations", async ({ page }) => {
  await workspace(page);
  let state = "blocked", offline = false, recovered = false;
  const message = () => state === "retry_wait" ? "Ожидается автоматический повтор" : state === "stalled" ? "Координатор не подаёт сигнал" : "Автоматическая обработка выключена · повтор не подтверждён";
  const posts: string[] = [];
  page.on("request", request => { if (request.method() === "POST") posts.push(request.url()); });
  await page.route("**/api/jobs", route => route.fulfill({ json: { running: 0, pending: 1, failed: 2 } }));
  await page.route("**/api/queue", route => offline ? route.fulfill({ status: 503, json: { error: "synthetic queue offline" } }) : route.fulfill({ json: { queue: [
    { ...fixture.queue[0], title: "Сводка архитектурной встречи", state: recovered ? "pending" : "failed", attempts: 3, phase: "summary", stuck: false, last_error: "Ollama временно недоступна", retry: recovered ? null : { state, message: message(), retry_at: "2026-10-31T08:00:00Z" } },
    { ...fixture.queue[0], session: "exhausted", title: "Запись с исчерпанным бюджетом", state: "failed", phase: "transcribe", attempts: 3, stuck: true, retry: { state: "failed", message: "Попытки автоматического повтора исчерпаны; требуется ручной повтор", retry_at: null } },
    { ...fixture.queue[0], session: "next", title: "Следующая запись", state: "pending", phase: "prepare", attempts: 0, position: 1, stuck: false, retry: null },
  ] } }));
  await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
  const row = page.locator(".qrow").first();
  await expect(row).toContainText("этап summary · попыток: 3");
  await expect(row).toContainText("Автоматическая обработка выключена");
  await expect(row).toContainText("Повтор не раньше: 31.10.2026, 11:00:00");
  await expect(page.locator(".qrow").nth(1)).not.toContainText("Повтор не раньше:");
  await expect(page.locator(".qrow").nth(1).getByRole("button", { name: "♻ Повторить этап", exact: true })).toBeVisible();
  await expect(page.locator(".main .pane")).toHaveScreenshot("queue-retry-plan-dark.png");
  state = "retry_wait"; await page.clock.runFor(3100);
  await expect(row.locator("[data-retry-state]")).toHaveAttribute("data-retry-state", "retry_wait");
  await expect(row).toContainText("ожидается повтор");
  state = "stalled"; await page.clock.runFor(3100);
  await expect(row).toContainText("Координатор не подаёт сигнал");
  offline = true; await page.clock.runFor(3100);
  await expect(page.getByText(/Очередь не обновляется/)).toBeVisible();
  await expect(row).toContainText("Повтор не раньше: 31.10.2026, 11:00:00");
  offline = false; recovered = true; await page.clock.runFor(6100);
  await expect(page.getByText(/Очередь не обновляется/)).toHaveCount(0);
  await expect(row).toHaveClass(/pending/);
  await expect(row).not.toContainText("Повтор не раньше:");
  expect(posts).toEqual([]);
});

test("queue legacy metadata never invents a phase, retry deadline or exhausted budget", async ({ page }) => {
  await workspace(page);
  let legacy = false;
  await page.route("**/api/queue", route => route.fulfill({ json: { queue: [{ ...fixture.queue[0], state: "failed", attempts: 2, stuck: false, last_error: "old failure", ...(!legacy ? { phase: "summary", retry: { state: "retry_wait", message: "Время отказа неизвестно; повтор возможен", retry_at: null } } : {}) }] } }));
  await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
  const row = page.locator(".qrow");
  await expect(row).toContainText("Срок повтора неизвестен");
  await expect(row).not.toContainText("Повтор не раньше:");
  await expect(row.getByRole("button", { name: "♻ Повторить этап", exact: true })).toHaveCount(0);
  legacy = true; await page.clock.runFor(3100);
  await expect(row).toContainText("этап неизвестен · попыток: 2");
  await expect(row).toContainText("повтор не подтверждён");
  await expect(row).not.toContainText("Повтор не раньше:");
});

test("queue retry resumes its failed phase without requesting a full recook", async ({ page }) => {
  await workspace(page);
  const posts: string[] = [];
  let accepted = false;
  await page.route("**/api/queue", route => route.fulfill({ json: { queue: [{ ...fixture.queue[0], state: accepted ? "pending" : "failed", stuck: !accepted }] } }));
  await page.route(/\/api\/sessions\/[^/]+\/(retry|recook)(\?.*)?$/, route => {
    posts.push(new URL(route.request().url()).pathname);
    accepted = true;
    return route.fulfill({ json: { msg: "Этап поставлен в очередь" } });
  });
  await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
  await page.getByRole("button", { name: /♻ Повторить/ }).click();
  await expect.poll(() => posts.length).toBe(1);
  expect(posts).toEqual(["/api/sessions/20261031_demo/retry"]);
  await expect(page.locator(".qrow")).toHaveClass(/pending/);
});

test("queue retry remains a single pending POST until the server acknowledges it", async ({ page }) => {
  await workspace(page);
  let posts = 0; let accepted = false;
  let release!: () => void; const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/queue", route => route.fulfill({ json: { queue: [{ ...fixture.queue[0], state: accepted ? "pending" : "failed", stuck: !accepted }] } }));
  await page.route("**/retry", async route => { posts++; await gate; accepted = true; await route.fulfill({ json: { msg: "Этап поставлен в очередь" } }); });
  await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
  await page.getByRole("button", { name: "♻ Повторить этап", exact: true }).click();
  const pending = page.getByRole("button", { name: "Постановка…", exact: true });
  await expect(pending).toBeDisabled();
  await pending.evaluate((node: HTMLButtonElement) => node.click());
  await page.clock.runFor(5100);
  expect(posts).toBe(1);
  await expect(page.locator(".qrow")).toHaveClass(/failed/);
  release();
  await expect(page.locator(".qrow")).toHaveClass(/pending/);
  expect(posts).toBe(1);
});

for (const status of [503, 404]) {
  test(`queue retry ${status} never falls back to full recook or loses the stopped job`, async ({ page }) => {
    await workspace(page);
    const posts: string[] = [];
    await page.route("**/api/queue", route => route.fulfill({ json: { queue: [{ ...fixture.queue[0], state: "failed", stuck: true, last_error: "previous failure" }] } }));
    await page.route(/\/api\/sessions\/[^/]+\/(retry|recook)(\?.*)?$/, route => {
      posts.push(new URL(route.request().url()).pathname);
      return route.fulfill({ status, json: { error: "retry unavailable" } });
    });
    await page.locator(".nav").getByRole("button", { name: /Очередь/ }).click();
    const retry = page.getByRole("button", { name: "♻ Повторить этап", exact: true });
    await retry.click();
    await expect(page.getByText("retry unavailable", { exact: true })).toBeVisible();
    await expect(retry).toBeEnabled();
    await expect(page.locator(".qrow")).toContainText("previous failure");
    await page.clock.runFor(5100);
    expect(posts).toEqual(["/api/sessions/20261031_demo/retry"]);
    await retry.click(); await expect.poll(() => posts.length).toBe(2);
    expect(posts.every(path => path.endsWith("/retry"))).toBe(true);
  });
}

test("an initial jobs read failure shows unavailable status without invented counters", async ({ page }) => {
  await workspace(page, { jobsError: true, record: true });
  const badge = page.locator(".bar > .queue.failed");
  await expect(badge).toHaveText("очередь недоступна");
  await expect(badge).toHaveAttribute("title", /synthetic queue unavailable/);
  const nav = page.locator(".nav").getByRole("button", { name: /Очередь/ });
  await expect(nav.locator(".k.err")).toHaveText("!");
  await page.setViewportSize({ width: 480, height: 1000 });
  await expect(nav.locator(".k.err")).toBeVisible();
  await expect(page.getByRole("button", { name: "Остановить", exact: true })).toBeVisible();
  await nav.click(); await expect(page.getByRole("heading", { name: "Очередь обработки", exact: true })).toBeVisible();
  await page.route("**/api/jobs", route => route.fulfill({ json: { running: 0, pending: 0, failed: 0 } }));
  await page.clock.runFor(10_100);
  await expect(badge).toHaveCount(0);
  await expect(nav.locator(".k.err")).toHaveCount(0);
});

test("jobs read failures label last counters and recovery publishes fresh status", async ({ page }) => {
  await workspace(page);
  let failed = true;
  await page.route("**/api/jobs", route => failed
    ? route.fulfill({ status: 503, json: { error: "synthetic jobs read failure" } })
    : route.fulfill({ json: { running: 0, pending: 4, failed: 2 } }));
  await page.clock.runFor(5100);
  const badge = page.locator(".bar > .queue.failed");
  await expect(badge).toContainText("очередь недоступна");
  await expect(badge).toContainText("последние данные: 1 в работе · 2 ждут · 0 ошибок");
  await expect(page.locator(".bar > .queue .qdot.busy")).toHaveCount(0);
  await expect(page.locator(".bar")).toHaveScreenshot("queue-unavailable-dark.png");
  failed = false; await page.clock.runFor(10_100);
  await expect(page.locator(".bar > .queue")).toContainText("сорвалось: 2");
  await expect(page.locator(".nav .k.err")).toHaveCount(0);
  await expect(page.locator(".nav").getByRole("button", { name: /Очередь/ })).toContainText("6");
});

test("current activity opens the real queue order and its selected session journal", async ({ page }) => {
  await workspace(page);
  await page.route("**/api/jobs", (route) => route.fulfill({ json: { running: 2, current: "20261031_demo", pending: 1, failed: 1 } }));
  await page.clock.runFor(5100);
  await expect(page.locator(".nav").getByRole("button", { name: /Очередь/ })).toContainText("4");
  const queue = [
    { ...fixture.queue[0], session: "z-running", title: "Вторая активная линия" },
    { ...fixture.queue[0] },
    { ...fixture.queue[0], session: "a-failed", title: "Остановленная задача", state: "failed", stuck: true, last_error: "Нет модели" },
  ];
  await page.route("**/api/queue", (route) => route.fulfill({ json: { queue } }));
  await page.getByRole("button", { name: /^обрабатывается/ }).click();
  await expect(page.locator(".qrow .qname b")).toHaveText(["Вторая активная линия", "Обсуждение архитектуры", "Остановленная задача"]);
  await expect(page.getByText("остановлено", { exact: true })).toBeVisible();
  await expect(page.getByText("Нет модели", { exact: true })).toBeVisible();
  await page.locator(".qrow .qname").nth(1).click();
  await expect(page.getByRole("button", { name: "+ 35 lines", exact: true })).toBeVisible();
  // A session without a reported stage must show waiting, rather than ready.
  await page.route("**/progress", (route) => route.fulfill({ json: { stages: [], running: false } }));
  await page.clock.runFor(3100);
  await expect(page.locator(".chain .waiting")).toHaveCount(4);
  await expect(page.locator(".chain .done")).toHaveCount(0);
});

for (const late of ["previous", "tail"] as const) {
  test(`file reset discards a late ${late} page and advances the new cursor`, async ({ page }) => {
    await workspace(page);
    let release!: () => void;
    const blocked = new Promise<void>((resolve) => { release = resolve; });
    let pending = false; let completed = false; let first = true; let newCursorRead = false;
    const event = (id: string) => ({ id, run: id, operation: "op", stage: "summary", kind: "log", at: "2026-10-31T07:00:20Z", label: id, text: id });
    const reset = { ...fixture.journal, events: [event("new-file")], reset: true, next_cursor: "new-tail", truncated: false };
    await page.route("**/journal**", async (route) => {
      const query = new URL(route.request().url()).searchParams;
      if (first) { first = false; return route.fulfill({ json: { ...fixture.journal, events: [event("old-file")], next_cursor: "old-tail", previous_cursor: "old-head", truncated: true } }); }
      if (query.get("after") === "new-tail") { newCursorRead = true; return route.fulfill({ json: { ...fixture.journal, next_cursor: "new-tail" } }); }
      const previous = query.has("before");
      if (previous === (late === "previous")) {
        pending = true; await blocked;
        await route.fulfill({ json: { ...fixture.journal, events: [event("stale-page")], next_cursor: "stale-tail" } });
        completed = true; return;
      }
      return route.fulfill({ json: reset });
    });
    await page.getByRole("button", { name: "Ход", exact: true }).click();
    const previous = page.getByRole("button", { name: "↑ Предыдущие события", exact: true });
    await expect(previous).toBeVisible();
    if (late === "tail") { await page.clock.runFor(2600); await expect.poll(() => pending).toBe(true); }
    await previous.click();
    if (late === "previous") { await expect.poll(() => pending).toBe(true); await page.clock.runFor(2600); }
    await expect(page.getByText("Файл истории сменился. Показаны события нового журнала.")).toBeVisible();
    await expect(page.locator('[data-journal-id="new-file/op"]')).toHaveCount(1);
    release(); await expect.poll(() => completed).toBe(true);
    await page.clock.runFor(2700);
    await expect.poll(() => newCursorRead).toBe(true);
    await expect(page.locator(".journal-operation")).toHaveCount(1);
    await expect(page.locator('[data-journal-id="stale-page/op"]')).toHaveCount(0);
    await expect(previous).toHaveCount(0);
  });
}

for (const width of [1440, 800, 480]) {
  test(`record stop remains visible at width ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await workspace(page, { record: true });
    const stop = page.getByRole("button", { name: "Остановить", exact: true });
    await expect(stop).toBeInViewport();
    await expect(page.locator(".list .item").first()).toBeInViewport();
    await expect(page.locator(".app")).toHaveScreenshot(`workspace-recording-${width}.png`);
  });
}

test("light theme and keyboard navigation keep stop and archive usable", async ({ page }) => {
  await page.setViewportSize({ width: 480, height: 900 });
  await workspace(page, { record: true });
  await page.getByTitle("Тема", { exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "Ход", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "+ 35 lines", exact: true })).toBeVisible();
  // Flush the initial follow-to-tail frame while the journal is still folded.
  await page.clock.runFor(50);
  await page.getByRole("button", { name: "+ 35 lines", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "− свернуть", exact: true })).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByRole("button", { name: "Остановить", exact: true })).toBeInViewport();
  await expect(page.locator(".app")).toHaveScreenshot("workspace-light-keyboard-480.png");
});

test("system theme follows OS and reduced motion disables activity animation", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  await workspace(page, { record: true });
  await expect(page.locator("html")).not.toHaveAttribute("data-theme");
  await expect.poll(() => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--ground").trim())).toBe("#f3f2ee");
  await expect(page.locator(".dot").first()).toHaveCSS("animation-name", "none");
  await expect(page.locator(".app")).toHaveScreenshot("workspace-system-light-1440.png");
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await expect.poll(() => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--ground").trim())).toBe("#17191d");
});

// Expected failures prove that the stored visual contract catches these changes.
// They compare to the reviewed normal baseline; they never update it.
for (const mutation of ["symbol", "color", "geometry"] as const) {
  test(`visual detector rejects changed ${mutation}`, async ({ page }, testInfo) => {
    test.skip(testInfo.config.updateSnapshots !== "none", "Never overwrite the normal baseline with an intentional mutation");
    await page.setViewportSize({ width: 1440, height: 900 });
    await workspace(page, { record: true });
    if (mutation === "symbol") await page.locator("h1,h2").evaluateAll((nodes) => nodes.forEach((node) => { node.textContent = "████████████████████████"; }));
    if (mutation === "color") await page.addStyleTag({ content: ".app { --surface: #ff00ff; --ground: #ff00ff; --side: #ff00ff; }" });
    if (mutation === "geometry") await page.addStyleTag({ content: ".app { grid-template-columns: 360px minmax(0, 1fr) 200px; }" });
    test.fail(true, "The deliberately mutated rendering must differ from the reviewed baseline");
    await expect(page.locator(".app")).toHaveScreenshot("workspace-recording-1440.png", { timeout: 1000 });
  });
}

function growingInfo(revision:number) {
  return {generation:"capture-g1",revision,snapshot:String(revision).repeat(64),sealed_end_sec:revision*100,stt_end_sec:revision*100,text_end_sec:revision*100,audio_end_sec:revision*100};
}
function growingLines(count:number) {
  return Array.from({length:count},(_,i)=>({id:`capture-g1/0/${i}`,who:"Микрофон",source_id:0,start_sec:i,end_sec:i+1,text:`Готовая реплика ${i}. Сохраняем выбранный фрагмент и читаем запись дальше.`}));
}
test("growing partial text preserves selection scroll and playing snapshot through stale replies and finalization",async({page})=>{
  await page.setViewportSize({width:1000,height:650});
  await page.addInitScript(()=>{
    Object.defineProperty(HTMLMediaElement.prototype,"currentTime",{get(){return (this as HTMLMediaElement & {testTime?:number}).testTime ?? 0;},set(value:number){(this as HTMLMediaElement & {testTime?:number}).testTime=value;}});
    HTMLMediaElement.prototype.load=function(){queueMicrotask(()=>this.dispatchEvent(new Event("loadedmetadata")));};
    HTMLMediaElement.prototype.play=function(){this.dispatchEvent(new Event("play"));return Promise.resolve();};
    HTMLMediaElement.prototype.pause=function(){this.dispatchEvent(new Event("pause"));};
  });
  await workspace(page,{partial:true});
  let revision=1;let final=false;
  await page.route("**/processed",route=>route.fulfill({json:{...fixture.readable,from_transcript:true,lines:growingLines(revision===1?80:120),partial:final?null:growingInfo(revision)}}));
  await page.route("**/playback",route=>route.fulfill({json:{duration_sec:revision*100,snapshot:final?null:growingInfo(revision).snapshot,partial:final?null:growingInfo(revision)}}));
  await page.getByRole("button",{name:"Текст",exact:true}).click();
  await expect(page.locator("[data-plain-text] span")).toHaveCount(80);
  await page.getByTitle("Слушать",{exact:true}).click();
  await page.locator("audio").evaluate(el=>{el.currentTime=50;el.dispatchEvent(new Event("timeupdate"));});
  const oldSource=await page.locator("audio").getAttribute("src");
  await page.locator(".main").evaluate(el=>{el.scrollTop=220;});
  const scroll=await page.locator(".main").evaluate(el=>el.scrollTop);
  const selected=await page.locator("[data-plain-text] span").nth(8).evaluate(el=>{
    const range=document.createRange();range.setStart(el.lastChild!,2);range.setEnd(el.lastChild!,15);const selection=getSelection()!;selection.removeAllRanges();selection.addRange(range);return selection.toString();
  });
  revision=2;await page.clock.runFor(3100);
  await expect(page.locator("[data-plain-text] span")).toHaveCount(120);
  expect(await page.evaluate(()=>getSelection()?.toString())).toBe(selected);
  expect(await page.locator(".main").evaluate(el=>el.scrollTop)).toBe(scroll);
  expect(await page.locator("audio").getAttribute("src")).toBe(oldSource);
  expect(await page.locator("audio").evaluate(el=>el.currentTime)).toBe(50);
  await expect(page.getByTitle("Пауза",{exact:true})).toBeVisible();
  revision=1;await page.clock.runFor(3100);await expect(page.locator("[data-plain-text] span")).toHaveCount(120);
  revision=2;
  const copied:string[]=[];await page.exposeFunction("captureGrowingCopy",(text:string)=>{copied.push(text);});
  await page.evaluate(()=>Object.defineProperty(navigator,"clipboard",{value:{writeText:(text:string)=>(window as unknown as {captureGrowingCopy:(s:string)=>Promise<void>}).captureGrowingCopy(text)}}));
  await page.keyboard.press("Control+Shift+C");await expect.poll(()=>copied.length).toBe(1);expect(copied[0]).toBe(growingLines(120).map(l=>l.text).join(" "));
  await page.getByTitle(/Доступно новое аудио/).click();
  await expect(page.locator("audio")).not.toHaveAttribute("src",oldSource!);
  expect(await page.locator("audio").evaluate(el=>el.currentTime)).toBe(50);
  await expect(page.getByTitle("Пауза",{exact:true})).toBeVisible();
  await page.locator(".main").evaluate(el=>{el.scrollTop=0;});
  await expect(page.locator(".main")).toHaveScreenshot("partial-growing-text.png");
  final=true;await page.clock.runFor(3100);
  await expect(page.locator(".doc")).not.toContainText("Готовая часть");
  await expect(page.locator("[data-plain-text] span")).toHaveCount(120);
  expect(await page.locator("audio").evaluate(el=>el.currentTime)).toBe(50);
});
