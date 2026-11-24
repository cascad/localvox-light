//! Pure bounded outline. Raw facts remain in Journal::groups; fold state never
//! changes their identity, duration or artifact outcome.
use super::*;
use localvox_light_client::Progress;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeKind {
    Context,
    Stage,
    Skipped,
    Timing,
    Details,
    #[default]
    Operation,
}

pub fn stage_name(stage: &str) -> &str {
    match stage {
        "download" => "Скачивание источника",
        "extract" => "Подготовка аудио",
        "transcribe" => "Расшифровка",
        "punctuation" => "Пунктуация",
        "refine" => "Правка текста",
        "cleanup" => "Очистка текста",
        "summary" => "Сводка",
        _ => stage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Journal, Progress) {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/journal-outline.json"
        ))
        .unwrap();
        let journal = Journal {
            entries: serde_json::from_value(value["events"].clone()).unwrap(),
            ..Journal::new()
        };
        (
            journal,
            serde_json::from_value(value["progress"].clone()).unwrap(),
        )
    }
    #[test]
    fn mixed_history_has_one_stage_result_and_retains_all_nested_timers_and_commands() {
        let (journal, progress) = fixture();
        let rows = journal.view(&progress);
        assert_eq!(
            rows.iter()
                .filter(|g| g.kind == NodeKind::Context)
                .map(|g| g.label.as_str())
                .collect::<Vec<_>>(),
            ["Во время записи", "После остановки"]
        );
        assert_eq!(
            rows.iter()
                .filter(|g| g.kind == NodeKind::Stage)
                .map(|g| g.stage.as_str())
                .collect::<Vec<_>>(),
            ["transcribe", "punctuation", "summary"]
        );
        assert!(rows.iter().all(|g| g.kind != NodeKind::Operation));
        assert!(rows[0].issues > 0, "a folded early failure remains visible");
        let stage = rows.iter().find(|g| g.stage == "summary").unwrap();
        assert_eq!(stage.elapsed_sec, Some(58.0));
        let copy = journal.copy_group(&stage.id, &progress);
        for fragment in [
            "final-run/answer",
            "54.000",
            "0.500",
            "1.000",
            "--worker-phase",
            "Проверяемая диагностическая строка",
        ] {
            assert!(copy.contains(fragment), "{fragment}");
        }
        assert!(!copy.contains("early-failed"));
        let all = journal.outline(&progress);
        let child = all
            .iter()
            .find(|g| g.operation.as_deref() == Some("early-punctuation"))
            .unwrap();
        assert_eq!(child.elapsed_sec, Some(24.0));
        assert!(
            child.depth
                > all
                    .iter()
                    .find(|g| g.operation.as_deref() == Some("early-prefix"))
                    .unwrap()
                    .depth
        );
    }
    #[test]
    fn filter_reveals_ancestors_without_mutating_folds_and_late_parent_keeps_identity() {
        let (mut journal, progress) = fixture();
        journal.filter = "Проверяемая диагностическая".into();
        let rows = journal.view(&progress);
        assert_eq!(
            rows.iter().map(|g| g.kind).collect::<Vec<_>>(),
            [NodeKind::Context, NodeKind::Stage, NodeKind::Operation]
        );
        assert_eq!(rows.last().unwrap().id, "final-run/answer");
        assert!(journal.expanded.is_empty() && journal.collapsed.is_empty());
        journal
            .entries
            .retain(|e| e.operation.as_deref() != Some("summary-worker"));
        assert_eq!(
            journal.view(&progress).last().unwrap().id,
            "final-run/answer"
        );
        assert!(journal
            .view(&progress)
            .last()
            .unwrap()
            .note
            .as_deref()
            .unwrap()
            .contains("вне загруженного окна"));
    }
    #[test]
    fn unknown_context_and_cycles_never_drop_or_misclassify_operations() {
        let (mut journal, _) = fixture();
        for e in &mut journal.entries {
            e.scope = None;
            e.role = None;
        }
        let rows = journal.outline(&Progress::default());
        assert!(rows
            .iter()
            .filter(|g| g.kind == NodeKind::Context)
            .all(|g| g.label.contains("контекст не записан")));
        journal.entries = vec![
            JournalEntry {
                run: "r".into(),
                operation: Some("a".into()),
                parent: Some("b".into()),
                ..Default::default()
            },
            JournalEntry {
                run: "r".into(),
                operation: Some("b".into()),
                parent: Some("a".into()),
                ..Default::default()
            },
        ];
        let rows = journal.outline(&Progress::default());
        assert_eq!(
            rows.iter()
                .filter(|g| g.kind == NodeKind::Operation)
                .count(),
            2
        );
        assert!(rows
            .iter()
            .filter(|g| g.kind == NodeKind::Operation)
            .all(|g| g.note.is_some()));
    }
    #[test]
    fn import_transport_does_not_become_a_second_prepare_stage() {
        let journal = Journal {
            entries: vec![
                JournalEntry {
                    id: "worker".into(),
                    run: "r".into(),
                    scope: Some("import".into()),
                    role: Some("worker".into()),
                    operation: Some("worker".into()),
                    stage: "prepare".into(),
                    kind: "operation_start".into(),
                    ..Default::default()
                },
                JournalEntry {
                    id: "download".into(),
                    run: "r".into(),
                    scope: Some("import".into()),
                    operation: Some("download".into()),
                    parent: Some("worker".into()),
                    stage: "download".into(),
                    kind: "operation_start".into(),
                    ..Default::default()
                },
            ],
            ..Journal::new()
        };
        let all = journal.outline(&Progress::default());
        assert_eq!(
            all.iter()
                .filter(|g| g.kind == NodeKind::Stage)
                .map(|g| g.stage.as_str())
                .collect::<Vec<_>>(),
            ["download"]
        );
        assert_eq!(
            all.iter()
                .find(|g| g.id == "r/download")
                .unwrap()
                .parent
                .as_deref(),
            Some("r/import/stage:download")
        );
        assert!(journal
            .copy_group("context:r/import", &Progress::default())
            .contains("r/worker"));
    }
}
fn context(run: &str, scope: Option<&str>) -> String {
    format!("context:{run}/{}", scope.unwrap_or("unknown"))
}
fn stage_id(run: &str, scope: Option<&str>, stage: &str) -> String {
    format!(
        "{run}/{}stage:{stage}",
        scope.map(|s| format!("{s}/")).unwrap_or_default()
    )
}
fn worker(g: &Group) -> bool {
    g.role.as_deref() == Some("worker")
        || g.command
            .as_ref()
            .is_some_and(|c| c.program == "localvox-process")
}
fn phase_wrapper(g: &Group) -> bool {
    worker(g)
        && ![
            "download",
            "extract",
            "transcribe",
            "punctuation",
            "refine",
            "cleanup",
            "summary",
        ]
        .contains(&g.stage.as_str())
}

impl Journal {
    pub fn outline(&self, progress: &Progress) -> Vec<Group> {
        let raw = self.groups();
        let raw_by_id = raw
            .iter()
            .map(|g| (g.id.as_str(), g))
            .collect::<HashMap<_, _>>();
        let current_run = progress
            .execution
            .as_ref()
            .and_then(|e| e.run.as_deref())
            .or_else(|| (!progress.stages.is_empty()).then_some("@planned"));
        let current_scope = raw
            .iter()
            .rev()
            .find(|g| Some(g.run.as_str()) == current_run)
            .and_then(|g| g.scope.as_deref());
        let mut nodes: Vec<Group> = Vec::new();
        let mut indices = HashMap::<String, usize>::new();
        let add_context = |nodes: &mut Vec<Group>,
                           indices: &mut HashMap<String, usize>,
                           run: &str,
                           scope: Option<&str>| {
            let id = context(run, scope);
            if !indices.contains_key(&id) {
                let ordinal = nodes.iter().filter(|g| g.kind == NodeKind::Context).count() + 1;
                let label = match scope {
                    _ if run == "@planned" => "План обработки".to_owned(),
                    Some("recording") => "Во время записи".to_owned(),
                    Some("final") => "После остановки".to_owned(),
                    Some("import") => "Импорт".to_owned(),
                    _ => format!("Проход {ordinal} · контекст не записан"),
                };
                indices.insert(id.clone(), nodes.len());
                nodes.push(Group {
                    id,
                    run: run.into(),
                    scope: scope.map(str::to_owned),
                    label,
                    kind: NodeKind::Context,
                    default_open: scope != Some("recording"),
                    ..Default::default()
                });
            }
        };
        for g in &raw {
            add_context(&mut nodes, &mut indices, &g.run, g.scope.as_deref());
        }
        if let Some(run) = current_run {
            add_context(&mut nodes, &mut indices, run, current_scope);
        }
        for g in &raw {
            let header = &mut nodes[indices[&context(&g.run, g.scope.as_deref())]];
            if let Ok(at) = chrono::DateTime::parse_from_rfc3339(&g.first_at) {
                if chrono::DateTime::parse_from_rfc3339(&header.first_at)
                    .map_or(true, |first| at < first)
                {
                    header.first_at = g.first_at.clone();
                }
            }
            if let Ok(at) = chrono::DateTime::parse_from_rfc3339(&g.at) {
                if chrono::DateTime::parse_from_rfc3339(&header.at).map_or(true, |last| at > last) {
                    header.at = g.at.clone();
                }
            }
        }
        // Stage rows are the single visible artifact outcome, not worker exit codes.
        for g in &raw {
            if phase_wrapper(g) {
                continue;
            }
            let id = stage_id(&g.run, g.scope.as_deref(), &g.stage);
            if !indices.contains_key(&id) {
                indices.insert(id.clone(), nodes.len());
                nodes.push(Group {
                    id: id.clone(),
                    run: g.run.clone(),
                    scope: g.scope.clone(),
                    stage: g.stage.clone(),
                    label: stage_name(&g.stage).into(),
                    kind: NodeKind::Stage,
                    parent: Some(context(&g.run, g.scope.as_deref())),
                    status: "events".into(),
                    ..Default::default()
                });
            }
            if g.operation.is_none() {
                let node = &mut nodes[indices[&id]];
                node.status = if g.scope.as_deref() == Some("recording") {
                    "events".into()
                } else {
                    g.status.clone()
                };
                node.at = g.at.clone();
                node.details = g.details.clone();
                node.done = g.done;
                node.total = g.total;
                node.started_at = g.started_at.clone();
                node.ended_at = g.ended_at.clone();
            }
        }
        if let Some(run) = current_run.filter(|_| current_scope != Some("recording")) {
            for stage in &progress.stages {
                let id = stage_id(run, current_scope, &stage.stage);
                if !indices.contains_key(&id) {
                    indices.insert(id.clone(), nodes.len());
                    nodes.push(Group {
                        id: id.clone(),
                        run: run.into(),
                        scope: current_scope.map(str::to_owned),
                        stage: stage.stage.clone(),
                        kind: NodeKind::Stage,
                        parent: Some(context(run, current_scope)),
                        label: stage_name(&stage.stage).into(),
                        ..Default::default()
                    });
                }
                let node = &mut nodes[indices[&id]];
                node.status = stage.state.clone();
                node.started_at = stage.started_at.clone();
                node.ended_at = stage.ended_at.clone();
                node.at = stage
                    .ended_at
                    .clone()
                    .or(stage.updated_at.clone())
                    .unwrap_or_default();
                node.done = stage.done;
                node.total = stage.total;
                node.note = stage.note.clone();
                node.elapsed_sec = stage
                    .started_at
                    .as_deref()
                    .zip(stage.ended_at.as_deref())
                    .and_then(|(start, end)| {
                        let start = chrono::DateTime::parse_from_rfc3339(start).ok()?;
                        let end = chrono::DateTime::parse_from_rfc3339(end).ok()?;
                        (end >= start).then(|| (end - start).num_milliseconds() as f64 / 1000.0)
                    });
            }
        }
        // A missing or out-of-page parent remains visible rather than changing
        // ownership to a coincidentally matching operation in another scope.
        let workers = raw
            .iter()
            .filter(|g| worker(g))
            .map(|g| {
                (
                    g.id.clone(),
                    if phase_wrapper(g) {
                        context(&g.run, g.scope.as_deref())
                    } else {
                        stage_id(&g.run, g.scope.as_deref(), &g.stage)
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let mut intervals: HashMap<String, Option<Vec<(i64, i64)>>> = HashMap::new();
        for g in raw.iter().filter(|g| workers.contains_key(&g.id)) {
            let list = intervals
                .entry(context(&g.run, g.scope.as_deref()))
                .or_insert_with(|| Some(Vec::new()));
            let bounds = g
                .started_at
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .zip(
                    g.ended_at
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()),
                );
            if let Some((start, end)) = bounds.filter(|(start, end)| end >= start) {
                if let Some(list) = list {
                    list.push((start.timestamp_millis(), end.timestamp_millis()));
                }
            } else {
                *list = None;
            }
        }
        for (id, list) in intervals {
            if let Some(mut list) = list {
                list.sort_unstable();
                let mut boundary = i64::MIN;
                let mut total = 0;
                for (start, end) in list {
                    total += (end - start.max(boundary)).max(0);
                    boundary = boundary.max(end);
                }
                nodes[indices[&id]].elapsed_sec = Some(total as f64 / 1000.0);
            }
        }
        for g in &raw {
            if g.operation.is_none() {
                continue;
            }
            let stage = stage_id(&g.run, g.scope.as_deref(), &g.stage);
            if workers.contains_key(&g.id) {
                let mut worker = g.clone();
                worker.source_parent = g.parent.clone();
                worker.parent = Some(format!("{}/details", workers[&g.id]));
                indices.insert(worker.id.clone(), nodes.len());
                nodes.push(worker);
                continue;
            }
            let mut node = g.clone();
            node.source_parent = g.parent.clone();
            let parent = g.parent.as_ref().map(|p| format!("{}/{}", g.run, p));
            node.parent = parent
                .as_ref()
                .and_then(|p| workers.get(p).filter(|id| **id == stage).cloned())
                .or_else(|| {
                    parent.filter(|p| {
                        raw_by_id.get(p.as_str()).is_some_and(|other| {
                            other.scope == g.scope && other.stage == g.stage && other.id != g.id
                        })
                    })
                })
                .or(Some(stage));
            if g.parent
                .as_deref()
                .is_some_and(|p| !p.starts_with("stage:"))
                && !g
                    .parent
                    .as_ref()
                    .is_some_and(|p| raw_by_id.contains_key(format!("{}/{p}", g.run).as_str()))
            {
                node.note = Some("Родительская операция вне загруженного окна".into());
            }
            indices.insert(node.id.clone(), nodes.len());
            nodes.push(node);
        }
        // Optional manual cleaners are one folded branch, not two promised steps.
        let skipped = nodes
            .iter()
            .filter(|g| {
                g.kind == NodeKind::Stage
                    && g.status == "skipped"
                    && matches!(g.stage.as_str(), "refine" | "cleanup")
            })
            .map(|g| g.id.clone())
            .collect::<Vec<_>>();
        for id in skipped {
            let index = indices[&id];
            let parent = nodes[index].parent.clone().unwrap();
            let group_id = format!("{parent}/not-requested");
            if !indices.contains_key(&group_id) {
                indices.insert(group_id.clone(), nodes.len());
                nodes.push(Group {
                    id: group_id.clone(),
                    parent: Some(parent),
                    kind: NodeKind::Skipped,
                    label: "Не запускались".into(),
                    status: "skipped".into(),
                    ..Default::default()
                });
            }
            nodes[index].parent = Some(group_id);
        }
        let worker_stages = workers.values().cloned().collect::<HashSet<_>>();
        let technical = nodes
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                matches!(g.kind, NodeKind::Stage | NodeKind::Context)
                    && (!g.details.is_empty()
                        || g.command.is_some()
                        || worker_stages.contains(&g.id))
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        for i in technical {
            let stage = &mut nodes[i];
            let details = std::mem::take(&mut stage.details);
            let command = stage.command.take();
            let node = Group {
                id: format!("{}/details", stage.id),
                run: stage.run.clone(),
                scope: stage.scope.clone(),
                stage: stage.stage.clone(),
                parent: Some(stage.id.clone()),
                label: if stage.kind == NodeKind::Context {
                    "Логи запуска"
                } else {
                    "Логи и команда"
                }
                .into(),
                kind: NodeKind::Details,
                details,
                command,
                ..Default::default()
            };
            nodes.push(node);
        }
        if let Some(timing) = &progress.timing {
            let duration = |seconds: Option<f64>| {
                seconds
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .map(|s| {
                        let v = (s * 10.0).round() as u64;
                        format!("{}:{:02}.{}", v / 600, v / 10 % 60, v % 10)
                    })
                    .unwrap_or_else(|| "неизвестно".into())
            };
            let mut details = vec![JournalEntry {
                text: Some(format!(
                    "Очередь: {} · после старта: {} · всего: {}",
                    duration(timing.queue_sec),
                    duration(timing.processing_sec),
                    duration(timing.total_sec)
                )),
                ..Default::default()
            }];
            details.push(JournalEntry {
                text: Some(
                    if let (Some(stages), Some(processing)) =
                        (timing.stages_sec, timing.processing_sec)
                    {
                        let rest = ((processing * 10.0).round() - (stages * 10.0).round()) / 10.0;
                        format!(
                            "Измеренная работа: {} + вне измеренных этапов: {} = после старта: {}",
                            duration(Some(stages)),
                            duration(Some(rest)),
                            duration(Some(processing))
                        )
                    } else {
                        "Точный баланс неизвестен: не все границы работы измерены".into()
                    },
                ),
                ..Default::default()
            });
            details.push(JournalEntry { text: Some("Вложенные операции входят во время родителя; пересечения учитываются один раз. Время материала — отдельно от обработки.".into()), ..Default::default() });
            nodes.push(Group {
                id: "processing-clock".into(),
                label: "Учёт времени".into(),
                kind: NodeKind::Timing,
                details,
                ..Default::default()
            });
        }
        // Repair only malformed presentation edges; keep original parent IDs in
        // the copied facts. Every operation remains reachable from its stage.
        let links = nodes
            .iter()
            .enumerate()
            .map(|(i, g)| (g.id.clone(), i))
            .collect::<HashMap<_, _>>();
        let mut resolved = HashSet::new();
        for start in 0..nodes.len() {
            if resolved.contains(&start) {
                continue;
            }
            let mut path = Vec::<usize>::new();
            let mut seen = HashMap::new();
            let mut current = start;
            while !resolved.contains(&current) {
                if let Some(&offset) = seen.get(&current) {
                    for &i in &path[offset..] {
                        nodes[i].note = Some("Циклическая ссылка родительских операций".into());
                    }
                    let g = &mut nodes[current];
                    g.parent = Some(stage_id(&g.run, g.scope.as_deref(), &g.stage));
                    break;
                }
                seen.insert(current, path.len());
                path.push(current);
                let Some(parent) = nodes[current]
                    .parent
                    .as_ref()
                    .and_then(|id| links.get(id))
                    .copied()
                else {
                    break;
                };
                current = parent;
            }
            resolved.extend(path);
        }
        let mut children = HashMap::<String, Vec<usize>>::new();
        for (i, node) in nodes.iter().enumerate() {
            if let Some(parent) = &node.parent {
                children.entry(parent.clone()).or_default().push(i);
            }
        }
        for list in children.values_mut() {
            if list
                .iter()
                .all(|i| matches!(nodes[*i].kind, NodeKind::Stage | NodeKind::Skipped))
            {
                list.sort_by_key(|i| {
                    [
                        "download",
                        "extract",
                        "transcribe",
                        "punctuation",
                        "refine",
                        "cleanup",
                        "summary",
                    ]
                    .iter()
                    .position(|name| *name == nodes[*i].stage)
                    .unwrap_or(99)
                });
            }
        }
        for i in 0..nodes.len() {
            if nodes[i].kind == NodeKind::Skipped {
                let names = children
                    .get(&nodes[i].id)
                    .into_iter()
                    .flatten()
                    .map(|i| nodes[*i].label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                nodes[i].label = format!("Не запускались · {names}");
            }
        }
        let mut ordered = Vec::new();
        let mut visited = HashSet::new();
        let mut stack = nodes
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, g)| g.parent.is_none())
            .map(|(i, _)| (i, 0))
            .collect::<Vec<_>>();
        while let Some((i, depth)) = stack.pop() {
            if !visited.insert(i) {
                continue;
            }
            let mut node = nodes[i].clone();
            node.depth = depth.min(32);
            if let Some(list) = children.get(&node.id) {
                node.children = list.len();
                stack.extend(list.iter().rev().map(|i| (*i, depth + 1)));
            }
            ordered.push(node);
        }
        // Malformed cycles/orphans are retained as clearly separate history rows.
        for (i, node) in nodes.into_iter().enumerate() {
            if !visited.contains(&i) {
                let mut node = node;
                node.depth = 2;
                node.note = Some("Родительская операция вне дерева".into());
                ordered.push(node);
            }
        }
        let ordered_indices = ordered
            .iter()
            .enumerate()
            .map(|(i, g)| (g.id.clone(), i))
            .collect::<HashMap<_, _>>();
        for i in (0..ordered.len()).rev() {
            let issues = usize::from(matches!(ordered[i].status.as_str(), "failed" | "invalid"))
                + ordered[i]
                    .details
                    .iter()
                    .filter(|e| matches!(e.status.as_deref(), Some("failed" | "invalid")))
                    .count();
            ordered[i].issues += issues;
            if let Some(parent) = ordered[i].parent.clone() {
                let issues = ordered[i].issues;
                if let Some(parent) = ordered_indices
                    .get(&parent)
                    .copied()
                    .filter(|parent| *parent < i)
                {
                    ordered[parent].issues += issues;
                }
            }
        }
        ordered
    }
    pub fn view(&self, progress: &Progress) -> Vec<Group> {
        let all = self.outline(progress);
        let parents = all
            .iter()
            .map(|g| (g.id.as_str(), g.parent.as_deref()))
            .collect::<HashMap<_, _>>();
        let filter = self.filter.to_lowercase();
        let mut keep = HashSet::new();
        if !filter.is_empty() {
            for node in &all {
                if format!("{} {} {}", node.label, node.stage, node.text())
                    .to_lowercase()
                    .contains(&filter)
                {
                    keep.insert(node.id.clone());
                    let mut parent = node.parent.as_deref();
                    for _ in 0..all.len() {
                        let Some(id) = parent else {
                            break;
                        };
                        if !keep.insert(id.into()) {
                            break;
                        }
                        parent = parents.get(id).copied().flatten();
                    }
                }
            }
        }
        let mut blocked_depth = None;
        all.into_iter()
            .filter(|g| {
                if !filter.is_empty() {
                    return keep.contains(&g.id);
                }
                if blocked_depth.is_some_and(|depth| g.depth > depth) {
                    return false;
                }
                blocked_depth = (!self.is_open(g) && g.children > 0).then_some(g.depth);
                true
            })
            .collect()
    }
    pub fn copy_group(&self, id: &str, progress: &Progress) -> String {
        let all = self.outline(progress);
        let Some(index) = all.iter().position(|g| g.id == id) else {
            return String::new();
        };
        let depth = all[index].depth;
        std::iter::once(&all[index])
            .chain(all[index + 1..].iter().take_while(|g| g.depth > depth))
            .map(Group::text)
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    pub fn reveal(&mut self, id: &str, progress: &Progress) {
        let all = self.outline(progress);
        let parents = all
            .iter()
            .map(|g| (g.id.as_str(), g.parent.as_deref()))
            .collect::<HashMap<_, _>>();
        let mut parent = parents.get(id).copied().flatten();
        for _ in 0..all.len() {
            let Some(id) = parent else {
                break;
            };
            self.set_open(id, true);
            parent = parents.get(id).copied().flatten();
        }
    }
    pub fn prune_folds(&mut self, progress: &Progress) {
        let active = self
            .outline(progress)
            .into_iter()
            .map(|g| g.id)
            .collect::<HashSet<_>>();
        self.expanded.retain(|id| active.contains(id));
        self.collapsed.retain(|id| active.contains(id));
    }
}
