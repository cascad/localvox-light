//! Feature operations behind the stable Archive API facade.
use super::*;

impl Archive {
    /// CREATE an ad-hoc «спросить у LLM» request — saved as `Pending` and returned AT ONCE. The
    /// model is NOT called here: that is the worker's job (see [`Self::process_ask`]). This is the
    /// whole point of the async rework — the button returns instantly, and the answer arrives in the
    /// background, surviving a navigation away or a daemon restart, exactly like a session's cook.
    pub fn create_ask(&self, req: AskRequest) -> Result<localvox_light_core::asks::Ask> {
        use localvox_light_core::asks::{self, Ask, AskStatus};

        // A cap that is generous for documents but keeps one request from swallowing memory or
        // blowing past the CLI's stdin limit (~10 MB). Well past a long PDF's text.
        const MAX_INPUT_CHARS: usize = 400_000;
        let input = req.text;
        anyhow::ensure!(
            !input.trim().is_empty(),
            "нечего отправлять: пустой материал"
        );
        anyhow::ensure!(
            input.chars().count() <= MAX_INPUT_CHARS,
            "материал слишком большой (> {MAX_INPUT_CHARS} символов) — сократите или разбейте"
        );

        let provider = req
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or("claude")
            .to_string();

        let now = chrono::Local::now();
        let ask = Ask {
            id: asks::new_id(now),
            status: AskStatus::Pending,
            created_at: now.to_rfc3339(),
            provider,
            model: None,
            prompt: asks::effective_prompt(req.prompt.as_deref()),
            input_kind: "text".into(),
            input_name: req.input_name.filter(|s| !s.trim().is_empty()),
            input_chars: input.chars().count(),
            answer: None,
            cost_usd: None,
            error: None,
        };
        asks::save(&self.asks_root(), &ask, &input).context("сохранение запроса в asks/")?;
        Ok(ask)
    }

    /// PROCESS one pending request — the worker's single step. Marks it `Running`, calls the model,
    /// then writes the answer (or the error) and the terminal status. A FAILURE is saved, not
    /// thrown: the person's material and the reason are kept, and the worker moves on.
    pub fn process_ask(&self, id: &str) -> Result<localvox_light_core::asks::Ask> {
        use localvox_light_core::asks::{self, AskStatus};
        let root = self.asks_root();
        let mut ask = asks::load(&root, id)?;
        let input = asks::input(&root, id)?;

        ask.status = AskStatus::Running;
        asks::update(&root, &ask).context("отметить запрос выполняющимся")?;

        // Dispatch. `claude` — the subscription via the official CLI (claude_cli). Anything else —
        // the OpenAI-compatible client (Ollama / Gemini free-tier / an API key), which the rest of
        // the product already speaks; no new provider code for those.
        let outcome: Result<(String, Option<f64>, Option<String>)> = if ask.provider == "claude" {
            let cfg = localvox_light_llm::claude_cli::ClaudeCliConfig::from_env();
            localvox_light_llm::claude_cli::run(&cfg, &ask.prompt, Some(&input))
                .map(|a| (a.text, a.cost_usd, cfg.model.clone()))
        } else {
            let client = llm_client_from_env_with(300);
            let model = client.model().to_string();
            client
                .chat(&[
                    localvox_light_llm::system(ask.prompt.clone()),
                    localvox_light_llm::user(input),
                ])
                .map(|text| (text, None, Some(model)))
        };

        match outcome {
            Ok((text, cost, model)) => {
                ask.answer = Some(text);
                ask.cost_usd = cost;
                ask.model = model;
                ask.error = None;
                ask.status = AskStatus::Done;
            }
            Err(e) => {
                ask.error = Some(format!("{e:#}"));
                ask.status = AskStatus::Failed;
            }
        }
        asks::update(&root, &ask).context("сохранение ответа")?;
        Ok(ask)
    }

    /// The oldest request still waiting — the worker's next unit of work. `None` — the queue is dry.
    pub fn next_pending_ask(&self) -> Option<String> {
        localvox_light_core::asks::pending(&self.asks_root())
            .into_iter()
            .next()
    }

    /// At startup, put any request left `Running` (the daemon died mid-call) back into the queue.
    pub fn reclaim_running_asks(&self) -> usize {
        localvox_light_core::asks::reclaim_running(&self.asks_root())
    }

    /// The list of past requests, newest first (for the section's sidebar).
    pub fn list_asks(&self) -> Vec<localvox_light_core::asks::AskSummary> {
        localvox_light_core::asks::list(&self.asks_root())
    }

    /// Delete one request entirely (input, answer, record). Returns the id so the caller can echo it.
    pub fn delete_ask(&self, id: &str) -> Result<String> {
        localvox_light_core::asks::delete(&self.asks_root(), id)?;
        Ok(id.to_string())
    }

    /// One request in full: the record plus the material it was made about.
    pub fn get_ask(&self, id: &str) -> Result<Value> {
        let ask = localvox_light_core::asks::load(&self.asks_root(), id)?;
        let input = localvox_light_core::asks::input(&self.asks_root(), id).unwrap_or_default();
        let mut v = serde_json::to_value(&ask)?;
        if let Value::Object(ref mut m) = v {
            m.insert("input".into(), Value::String(input));
        }
        Ok(v)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let index = SearchIndex::open_or_build(&self.work_dir, false)?;
        Ok(index
            .search(query, limit.clamp(1, 50))?
            .into_iter()
            .map(|h| Hit {
                session: h.session,
                kind: h.kind,
                start_sec: h.start_sec,
                end_sec: h.end_sec,
                snippet: h.snippet,
                score: None,
                matched: h.matched,
                why: WHY_WORDS.into(),
            })
            .collect())
    }

    /// Search with a mode: `lexical` (tantivy, default), `semantic` (vectors via
    /// Ollama), `hybrid` (RRF merge of both — exact words AND meaning).
    pub fn search_mode(&self, query: &str, limit: usize, mode: &str) -> Result<Vec<Hit>> {
        // Empty query: do not drive Ollama and do not return top-k "matches" for a
        // meaningless vector (parity with the localvox-search CLI).
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, 50);
        match mode {
            "semantic" => Ok(self.semantic_hits(query, limit)?),
            "hybrid" => {
                // Each channel yields more candidates than needed; RRF merge:
                // score = Σ 1/(60+rank) — the standard RRF constant.
                //
                // The lexical channel can fail too — and not only by "crashing": the
                // tantivy query parser rejects whole classes of queries ("Only
                // excluding terms given"), and the hybrid used to fail ENTIRELY
                // because of that, even though semantics would have answered the very
                // same question perfectly. Degradation must be symmetric: as long as
                // at least one channel is alive, search works.
                let lex = self.search(query, limit * 2);
                // Semantics is an optional module (Ollama may be turned off): the
                // hybrid degrades to lexical instead of failing entirely.
                let sem = self.semantic_hits(query, limit * 2);
                if let Err(error) = &lex {
                    tracing::warn!("hybrid: lexical unavailable: {error:#}");
                }
                if let Err(error) = &sem {
                    tracing::warn!("hybrid: semantics unavailable: {error:#}");
                }
                anyhow::ensure!(
                    lex.is_ok() || sem.is_ok(),
                    "поиск недоступен: лексический и семантический каналы завершились ошибкой"
                );
                let lex = lex.unwrap_or_default();
                let sem = sem.unwrap_or_default();
                let mut scored: Vec<(f64, Hit)> = Vec::new();
                // A hit that came from BOTH channels is the most reliable one: same
                // words and same meaning. A person needs to see that, not deduce it
                // from the ordering.
                let add = |hits: Vec<Hit>, scored: &mut Vec<(f64, Hit)>| {
                    // RRF: one contribution per channel per document (the best rank).
                    // Duplicates WITHIN a channel (summary and processed with the same
                    // paragraph) are merged for display but do not sum their scores —
                    // otherwise the md duplicate would unfairly outrank transcript
                    // hits.
                    let mut contributed: Vec<usize> = Vec::new();
                    for (rank, h) in hits.into_iter().enumerate() {
                        let rrf = 1.0 / (60.0 + rank as f64);
                        // dedup: same session + same line (start_sec)
                        if let Some(i) = scored.iter().position(|(_, e)| {
                            e.session == h.session
                                && match (e.start_sec, h.start_sec) {
                                    (Some(a), Some(b)) => (a - b).abs() < 0.5,
                                    (None, None) => e.snippet == h.snippet,
                                    _ => false,
                                }
                        }) {
                            if !contributed.contains(&i) {
                                contributed.push(i);
                                scored[i].0 += rrf;
                                if scored[i].1.why != h.why {
                                    scored[i].1.why = WHY_BOTH.into();
                                }
                                // We take the highlighting from wherever it exists:
                                // the semantic channel does not look for words, but it
                                // found the same line.
                                if scored[i].1.matched.is_empty() {
                                    scored[i].1.matched = h.matched.clone();
                                }
                            }
                        } else {
                            contributed.push(scored.len());
                            scored.push((rrf, h));
                        }
                    }
                };
                add(lex, &mut scored);
                add(sem, &mut scored);
                scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                Ok(scored.into_iter().take(limit).map(|(_, h)| h).collect())
            }
            _ => self.search(query, limit),
        }
    }

    pub(super) fn semantic_hits(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        // `open_or_update` (re)indexes synchronously under an exclusive lock: the
        // first request after a cook/refine may hold the thread for a long time, and
        // without a limit parallel requests would queue up on the lock and eat the
        // HTTP server's shared MAX_INFLIGHT — freezing the WHOLE API, including
        // /api/health (the same motive as RouteGuard for /api/route). On "busy" the
        // hybrid degrades to lexical (see search_mode).
        let _guard = SemanticGuard::try_acquire()
            .context("занято: семантический индекс обновляется, повторите")?;
        let idx = localvox_light_search::semantic::SemanticIndex::open_or_update(&self.work_dir)?;
        Ok(idx
            .search(query, limit)?
            .into_iter()
            .map(|h| Hit {
                session: h.session,
                // the same vocabulary of kinds as the lexical channel (transcript/
                // summary/processed) — the front end and the RRF dedup work uniformly
                kind: h.kind,
                start_sec: h.start_sec,
                end_sec: h.end_sec,
                // The closeness of meaning — the ONLY thing a human can judge a semantic hit
                // by: there are no words of his query in it.
                score: Some(h.score),
                // THE SAME formatter as the lexical channel: the RRF dedup of md hits
                // (they have start_sec=None) compares exactly the snippets — with
                // different formats one paragraph would be shown twice and would lose
                // the sum of its scores
                snippet: localvox_light_search::make_snippet(&h.text, query, 160),
                // We compute the matched words here as well: a semantic hit MAY contain
                // the query's words (then they are visible), and may contain none of
                // them — and then it is empty, which is the truth about this hit.
                matched: localvox_light_search::matched_terms(&h.text, query),
                why: WHY_MEANING.into(),
            })
            .collect())
    }

    /// A question to the archive (F4): an answer BASED ON THE FOUND FRAGMENTS, with
    /// citations to them. If nothing was found, the model is not called at all (see
    /// `chat::ask`).
    pub fn ask(&self, question: &str) -> Result<crate::chat::Answer> {
        // Temperature 0: the answer must not depend on a dice roll.
        //
        // And a CEILING on the answer. An answer about your own recordings is a few sentences —
        // the default 8192 tokens is a licence to ramble, and the model took it: asked «про код»,
        // it produced seventy lines of «в записях нет информации о том, как код связан с…»
        // (14.07.2026). The prompt asks for brevity; a ceiling enforces it.
        let client = llm_client_capped(
            env_secs("LOCALVOX_LLM_CHAT_TIMEOUT_SEC", 120),
            Some(0.0),
            env_secs("LOCALVOX_LLM_CHAT_MAX_TOKENS", 800) as u32,
        );
        crate::chat::ask(self, question, &client)
    }
}
