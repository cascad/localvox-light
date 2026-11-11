//! Feature operations behind the stable Archive API facade.
use super::*;

impl Archive {
    /// Slots for notes — only the configured ones: a choice from what is offered
    /// rather than free-form input (a typo in a slot name silently sent the note into
    /// the default file).
    pub fn slots(&self) -> Vec<String> {
        let config = SlotRegistry::default_config_path(None);
        SlotRegistry::load(&config)
            .map(|r| r.names().into_iter().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// Remove one note from a slot.
    ///
    /// This deletes from the person's OWN vault — a file they also edit by hand — so the note is
    /// identified by its text and its file, never by a position in a list that may already be
    /// stale. The slot's configured path is the boundary of what can be touched, and that is
    /// enforced inside the integration rather than trusted from here.
    /// `raw` is the line AS IT IS IN THE FILE — the screen shows a cleaned rendering (no bullet, no
    /// date prefix), and deleting by that rendering would either miss the line or match a different
    /// one that happens to render identically.
    pub fn delete_slot_note(&self, slot: &str, raw: &str, source: &str) -> Result<()> {
        let config = SlotRegistry::default_config_path(None);
        let registry = SlotRegistry::load(&config).context("не прочитан slots.toml")?;
        let target = registry
            .resolve(slot)
            .with_context(|| format!("слота «{slot}» нет"))?;
        target.delete_note(&localvox_light_integrations::StoredNote {
            text: raw.to_string(),
            date: None,
            raw: raw.to_string(),
            source: source.to_string(),
        })
    }

    /// The notes that are actually IN the slots — «полистать посмотреть».
    ///
    /// Until this existed, a voice note left for a file on another disk and the app never mentioned
    /// it again: the only way to check that anything had been written was a file manager. A
    /// secretary that takes dictation and then refuses to show you the notebook is half a
    /// secretary.
    ///
    /// A slot that cannot be read back (an MCP server we only send to) is reported as such, with
    /// its reason — never as an empty list, because «нельзя прочитать» and «здесь пусто» are
    /// opposite facts.
    pub fn slot_notes(&self, limit: usize) -> Vec<serde_json::Value> {
        let config = SlotRegistry::default_config_path(None);
        let Ok(registry) = SlotRegistry::load(&config) else {
            return Vec::new();
        };
        registry
            .slots()
            .iter()
            .map(|s| {
                let (notes, error) = if s.can_read() {
                    match s.read_notes(limit) {
                        Ok(n) => (n, None),
                        // A destination configured but unreachable — an unplugged drive, a path
                        // that moved. Saying which beats an empty list that reads as «нет идей».
                        Err(e) => (Vec::new(), Some(format!("{e:#}"))),
                    }
                } else {
                    (Vec::new(), Some("сюда можно только писать".to_string()))
                };
                json!({
                    "name": s.name,
                    "default": s.default,
                    "readable": s.can_read(),
                    "notes": notes,
                    "error": error,
                })
            })
            .collect()
    }

    /// F5 routing: suggest which slots to file the text into (via the LLM). The list
    /// of slots and the hints come from the registry; the LLM config — from the env
    /// (the same `LOCALVOX_LLM_*` as `localvox-process`). Existing slots only.
    pub fn route(&self, text: &str) -> Result<Vec<RouteHint>> {
        let config = SlotRegistry::default_config_path(None);
        let registry = SlotRegistry::load(&config)
            .with_context(|| format!("конфиг слотов: {}", config.display()))?;
        let slots: Vec<localvox_light_llm::routing::RouteSlot> = registry
            .slots()
            .iter()
            .map(|s| localvox_light_llm::routing::RouteSlot {
                name: s.name.clone(),
                // the description, otherwise the aliases — at least some hint for the
                // model
                hint: if s.description.trim().is_empty() {
                    s.aliases.join(", ")
                } else {
                    s.description.clone()
                },
            })
            .collect();
        let client = llm_client_from_env_with(env_secs("LOCALVOX_LLM_ROUTE_TIMEOUT_SEC", 30));
        let out = localvox_light_llm::routing::suggest_routing(text, &slots, &client)?;
        Ok(out
            .into_iter()
            .map(|s| RouteHint {
                slot: s.slot,
                reason: s.reason,
            })
            .collect())
    }

    pub fn append_note(&self, slot: Option<&str>, text: &str) -> Result<String> {
        let config = SlotRegistry::default_config_path(None);
        let registry = SlotRegistry::load(&config)
            .with_context(|| format!("конфиг слотов: {}", config.display()))?;
        let target = match slot {
            Some(q) => registry.resolve(q).with_context(|| {
                format!(
                    "слот «{q}» не найден; есть: {}",
                    registry.names().join(", ")
                )
            })?,
            None => registry.default_slot().context("нет ни одного слота")?,
        };
        let dest = target.write_note(text)?;
        Ok(format!("Записано в слот «{}»: {dest}", target.name))
    }
}
